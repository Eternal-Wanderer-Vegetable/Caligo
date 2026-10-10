//! 账号域消息存储(N4;计划 §7-N4)。
//!
//! 原生记录字段(sender/peer/text/seq/platform_time)→ 持久本地整数
//! message_id 的单向稳定映射:id 由账号内单调计数器分配,**不由
//! hash 截断或 request_id 冒充原生 ID**(计划 §7-N4);原生 seq 随记录
//! 保留,双向查询都走本结构。
//!
//! 接收侧来源 = native_receive 的 diff 原语(SeenRecords 吸收后的新记录);
//! 发送侧 = 自己发出的消息(记录含 sender=自 uid)。持久化挂接(journal)
//! 在 N4 后续交付;本结构先保证内存态合同与 ID 稳定语义。

use crate::onebot::StoreReader;

/// 一条已存储消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub message_id: i64,
    /// 群 = Some(群码);私聊 = peer uid/uin 串。
    pub peer: String,
    pub sender: String,
    pub text: String,
    pub seq: u64,
    /// 平台时间(Unix 秒;0x9c72/2)。
    pub platform_time: u64,
    /// 方向:true = 本账号发出。
    pub self_sent: bool,
}

/// 账号域消息存储(单线程所有;由账号 actor 持有)。
#[derive(Default)]
pub struct MessageStore {
    by_id: std::collections::BTreeMap<i64, StoredMessage>,
    /// (peer, seq) → message_id(接收去重与查询)。
    by_peer_seq: std::collections::HashMap<(String, u64), i64>,
    next_id: i64,
}

impl MessageStore {
    pub fn new(next_id: i64) -> Self {
        Self { by_id: std::collections::BTreeMap::new(), by_peer_seq: std::collections::HashMap::new(), next_id }
    }

    /// 插入一条消息;返回分配的 message_id。
    /// 同 (peer, seq) 重复插入幂等(返回已有 id,不产生第二条)。
    pub fn insert(
        &mut self,
        peer: &str,
        sender: &str,
        text: &str,
        seq: u64,
        platform_time: u64,
        self_sent: bool,
    ) -> i64 {
        if let Some(&id) = self.by_peer_seq.get(&(peer.to_string(), seq)) {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        let m = StoredMessage {
            message_id: id,
            peer: peer.to_string(),
            sender: sender.to_string(),
            text: text.to_string(),
            seq,
            platform_time,
            self_sent,
        };
        self.by_id.insert(id, m);
        self.by_peer_seq.insert((peer.to_string(), seq), id);
        id
    }

    pub fn get(&self, message_id: i64) -> Option<&StoredMessage> {
        self.by_id.get(&message_id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl StoreReader for MessageStore {
    fn get_msg(&self, message_id: i64) -> Option<(u64, u64, String)> {
        self.by_id
            .get(&message_id)
            .map(|m| (m.seq, m.platform_time, m.text.clone()))
    }
    fn self_id(&self) -> u64 {
        // 账号 ID 由外层注入;此处 0 = 未填充(OneBot 侧可见,不伪造)。
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_assigns_stable_local_ids() {
        let mut st = MessageStore::new(1000);
        let a = st.insert("263402786", "u_SELF", "N3R21-DONE", 398263, 1791621805, true);
        let b = st.insert("263402786", "u_OTHER", "hi", 398264, 1791621900, false);
        assert_eq!(a, 1000);
        assert_eq!(b, 1001, "本地整数 id 单调分配");
        assert_eq!(st.get(a).unwrap().text, "N3R21-DONE");
    }

    #[test]
    fn duplicate_peer_seq_is_idempotent() {
        let mut st = MessageStore::new(1);
        let a = st.insert("263402786", "u_X", "dup", 398263, 1791621805, false);
        let b = st.insert("263402786", "u_X", "dup", 398263, 1791621805, false);
        assert_eq!(a, b, "同 (peer, seq) 幂等");
        assert_eq!(st.len(), 1);
    }

    #[test]
    fn same_seq_across_peers_distinct() {
        let mut st = MessageStore::new(1);
        let a = st.insert("263402786", "u_X", "g", 100, 1, false);
        let b = st.insert("950238216", "u_Y", "p", 100, 1, false);
        assert_ne!(a, b, "seq 只在 peer 内唯一;跨 peer 各自成条");
    }

    #[test]
    fn store_reader_serves_onebot_get_msg() {
        let mut st = MessageStore::new(5);
        let id = st.insert("263402786", "u_X", "N3R21-DONE", 398263, 1791621805, true);
        let (seq, time, text) = StoreReader::get_msg(&st, id).unwrap();
        assert_eq!((seq, time, text), (398263, 1791621805, "N3R21-DONE".into()));
        assert!(StoreReader::get_msg(&st, 9999).is_none());
    }
}
