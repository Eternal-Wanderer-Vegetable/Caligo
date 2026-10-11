use caligo_core::message_store::{ChatType, MessageIdentity, MessageStore};
use caligo_core::onebot::{handle_action, StoreReader};
use caligo_core::onebot_http::{serve_one, DEFAULT_MAX_BODY};
use caligo_model::onebot_traits::{SendDisposition, SendPayload, SendSink, StoredMessageView};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct SharedStore(Arc<Mutex<MessageStore>>);

impl StoreReader for SharedStore {
    fn get_msg(&self, message_id: i32) -> Option<StoredMessageView> {
        self.0.lock().unwrap().get_msg(message_id)
    }

    fn login_info(&self) -> Option<(u64, String)> {
        self.0.lock().unwrap().login_info()
    }

    fn online_status(&self) -> Option<bool> {
        self.0.lock().unwrap().online_status()
    }
}

struct FakeDaemon {
    store: SharedStore,
    next_native_id: u64,
}

impl SendSink for FakeDaemon {
    fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
        let (chat_type, peer_id) = match payload.target {
            caligo_model::onebot_traits::SendTarget::Group { group_id } => {
                (ChatType::Group, group_id.to_string())
            }
            caligo_model::onebot_traits::SendTarget::Private { user_id } => {
                (ChatType::Private, user_id.to_string())
            }
        };
        self.next_native_id += 1;
        let message_id = self.store.0.lock().unwrap().insert_confirmed(
            MessageIdentity {
                account_id: 100,
                chat_type,
                peer_id,
                native_id: format!("native-{}", self.next_native_id),
            },
            self.next_native_id as i32,
            json!({"user_id": 100, "nickname": "lab-bot"}),
            json!(payload.text),
            1_791_692_800,
            Some(self.next_native_id),
        )?;
        Ok(SendDisposition::Confirmed { message_id })
    }
}

fn store_path() -> PathBuf {
    let id = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "caligo-onebot-http-lab-{}-{id}.jsonl",
        std::process::id()
    ))
}

fn request(addr: std::net::SocketAddr, method: &str, path: &str, body: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    let content_type = if method == "POST" {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: local\r\n{content_type}Content-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    String::from_utf8(response).unwrap()
}

fn response_body(response: &str) -> Value {
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

#[test]
fn http_send_persists_onebot_id_and_get_msg_survives_store_reopen() {
    let path = store_path();
    let initial = MessageStore::open(&path, 100, Some("lab-bot".into())).unwrap();
    let store = SharedStore(Arc::new(Mutex::new(initial)));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server_store = store.clone();
    let server_path = path.clone();

    let server = std::thread::spawn(move || {
        let mut daemon = FakeDaemon {
            store: server_store.clone(),
            next_native_id: 0,
        };
        for index in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            serve_one(stream, DEFAULT_MAX_BODY, None, |action, params, echo| {
                let reader = server_store.clone();
                handle_action(action, params, echo, &mut daemon, &reader)
            });
            if index == 0 {
                // Re-open exactly as the core would after restart before get_msg.
                let reopened =
                    MessageStore::open(&server_path, 100, Some("lab-bot".into())).unwrap();
                *server_store.0.lock().unwrap() = reopened;
            }
        }
    });

    let send = request(
        addr,
        "POST",
        "/send_group_msg",
        br#"{"group_id":263402786,"message":"hello from lab"}"#,
    );
    assert!(send.starts_with("HTTP/1.1 200"));
    let send_body = response_body(&send);
    assert_eq!(send_body["status"], "ok");
    assert_eq!(send_body["data"]["message_id"], 1);
    assert_eq!(send_body["echo"], Value::Null);

    let get = request(addr, "GET", "/get_msg?message_id=1", b"");
    assert!(get.starts_with("HTTP/1.1 200"));
    let get_body = response_body(&get);
    assert_eq!(get_body["status"], "ok");
    assert_eq!(get_body["data"]["message_id"], 1);
    assert_eq!(get_body["data"]["message_type"], "group");
    assert_eq!(get_body["data"]["real_id"], 1);
    assert_eq!(get_body["data"]["sender"]["user_id"], 100);
    assert_eq!(get_body["data"]["message"], "hello from lab");
    assert_eq!(get_body["data"]["time"], 1_791_692_800);
    server.join().unwrap();

    std::fs::remove_file(path).unwrap();
}
