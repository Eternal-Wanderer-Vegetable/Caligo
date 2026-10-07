p = 'crates/caligo-bridge/tests/qq_entry_lab.rs'
s = open(p, encoding='utf-8').read()
old = '''#[test]
fn adapter_rejects_unproven_ops_and_resident_deferred_mode() {
    let _g = TEST_LOCK.lock().unwrap();
    caligo_bridge::qq_entry::test_reset_state();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();'''
new = '''#[test]
fn adapter_listener_ops_via_v8_ladder_and_resident_full_mode() {
    let _g = TEST_LOCK.lock().unwrap();
    caligo_bridge::qq_entry::test_reset_state();
    install_fake_v8();
    fake_v8::CURRENT.store(0x1DEA_0001, Ordering::Release);
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();'''
assert old in s
s = s.replace(old, new)

old2 = '''    // Probe:通过(新鲜度 + current)。
    assert!(adapter.native_op(HostOp::Probe).is_ok());
    // D8/D9 面:显式拒绝 / 延迟,不冒充能力。
    assert_eq!(
        adapter.native_op(HostOp::ListenerAdd).unwrap(),
        HostOpResult::ListenerDeferred
    );
    assert!(matches!(
        adapter.native_op(HostOp::ListenerRemove { token: 1 }),
        Err(HostError::NoProvenRoute)
    ));
    assert!(matches!(
        adapter.native_op(HostOp::SendText { text_len: 1 }),
        Err(HostError::NoProvenRoute)
    ));

    // resident 以延迟监听模式 bootstrap/close(关闭不做对称移除)。
    let resident = Resident::new(adapter, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let report = resident.close().unwrap();
    assert_eq!(report.listener_removed, None, "延迟模式:无监听器可移除");
}'''
new2 = '''    // Probe:通过(新鲜度 + loop 存活)。
    assert!(adapter.native_op(HostOp::Probe).is_ok());
    // D8 监听器:经 V8 阶梯执行注册脚本 → ARMED → token 17。
    assert_eq!(
        adapter.native_op(HostOp::ListenerAdd).unwrap(),
        HostOpResult::ListenerAdded { token: 17 }
    );
    // 对称移除:STOPPED 确认。
    assert_eq!(
        adapter.native_op(HostOp::ListenerRemove { token: 17 }).unwrap(),
        HostOpResult::ListenerRemoved
    );
    // D9 发送:显式拒绝(未接线,不冒充)。
    assert!(matches!(
        adapter.native_op(HostOp::SendText { text_len: 1 }),
        Err(HostError::NoProvenRoute)
    ));

    // resident 全模式 bootstrap/close(注册 → 对称移除,资源回基线)。
    let resident = Resident::new(adapter, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let report = resident.close().unwrap();
    assert_eq!(report.listener_removed, Some(true), "对称移除并确认");
    let c = resident.counters();
    assert_eq!(c.listener_adds, 1);
    assert_eq!(c.listener_removes, 1);
}'''
assert old2 in s
s = s.replace(old2, new2)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
