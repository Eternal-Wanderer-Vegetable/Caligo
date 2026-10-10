//! OneBot 事件 fanout(N4;计划 §7-N4)。
//!
//! 有界订阅者(独立背压,计划 §9):每个订阅者一条有界 mpsc;发布时
//! try_send,满则**丢弃该订阅者的本条事件并计数**(不阻塞发布者、不阻塞
//! QQ 线程;丢弃数对订阅者可见)。订阅者断开(send 失败)自动注销。
//!
//! 事件内容 = JSON 文本(WS 推送 / HTTP POST 上报共用同一 fanout)。

use std::collections::HashMap;
use std::sync::mpsc;

/// 单订阅者队列上限(条)。
pub const SUBSCRIBER_QUEUE_CAP: usize = 256;

#[derive(Default)]
pub struct EventBus {
    next_id: u64,
    subscribers: HashMap<u64, (mpsc::SyncSender<String>, String, u64)>,
    /// 各订阅者累计丢弃数(诊断;随注销清除)。
    pub dropped_total: u64,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册订阅者(名称供诊断);返回接收端。
    pub fn subscribe(&mut self, name: &str) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::sync_channel::<String>(SUBSCRIBER_QUEUE_CAP);
        let id = self.next_id;
        self.next_id += 1;
        self.subscribers.insert(id, (tx, name.to_string(), 0));
        (id, rx)
    }

    pub fn unsubscribe(&mut self, id: u64) -> bool {
        self.subscribers.remove(&id).is_some()
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// 发布事件(JSON 文本)到全部订阅者;满队列丢弃并计数。
    /// 返回 (送达数, 丢弃数)。
    pub fn publish(&mut self, event_json: &str) -> (usize, u64) {
        let mut delivered = 0usize;
        let mut dropped = 0u64;
        let mut dead = Vec::new();
        for (id, (tx, _name, drops)) in self.subscribers.iter_mut() {
            match tx.try_send(event_json.to_string()) {
                Ok(()) => delivered += 1,
                Err(mpsc::TrySendError::Full(_)) => {
                    *drops += 1;
                    dropped += 1;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => dead.push(*id),
            }
        }
        for id in dead {
            self.subscribers.remove(&id);
        }
        self.dropped_total += dropped;
        (delivered, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_reaches_all_subscribers_with_echo_of_payload() {
        let mut bus = EventBus::new();
        let (id1, mut rx1) = bus.subscribe("ws-1");
        let (_id2, mut rx2) = bus.subscribe("http-post-1");
        let (d, dr) = bus.publish(r#"{"post_type":"message","raw_message":"hi"}"#);
        assert_eq!((d, dr), (2, 0));
        assert!(rx1.recv().unwrap().contains("hi"));
        assert!(rx2.recv().unwrap().contains("hi"));
        bus.unsubscribe(id1);
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[test]
    fn slow_subscriber_drops_without_blocking_and_dead_unsubscribes() {
        let mut bus = EventBus::new();
        let (_slow_id, _slow_rx) = bus.subscribe("slow");
        // slow 订阅者的接收端已 move 走但未读 → 填满后丢弃。
        let (_fast_id, mut fast_rx) = bus.subscribe("fast");
        let mut dropped_total = 0u64;
        for i in 0..(SUBSCRIBER_QUEUE_CAP + 10) {
            let ev = format!(r#"{{"i":{i}}}"#);
            let (_, dr) = bus.publish(&ev);
            dropped_total += dr;
            // fast 订阅者随取随清,不应丢弃。
            let _ = fast_rx.try_recv().unwrap();
        }
        assert!(dropped_total > 0, "慢订阅者超限必须丢弃并计数");
        assert_eq!(bus.dropped_total, dropped_total);
        // 断开的订阅者:发布时自动注销。
        drop(_slow_rx);
        let (d, _) = bus.publish(r#"{"i":x}"#);
        assert_eq!(d, 1, "仅 fast 收到");
        assert_eq!(bus.subscriber_count(), 1);
    }
}
