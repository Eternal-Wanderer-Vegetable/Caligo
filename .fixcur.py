p = 'crates/caligo-bridge/tests/qq_entry_lab.rs'
s = open(p, encoding='utf-8').read()
# adapter test: CURRENT must equal the FakeQq isolate address (ENTRY.isolate), not a constant
old = '''    lab_reset();
    install_fake_v8();
    fake_v8::CURRENT.store(0x1DEA_0001, Ordering::Release);
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("adapter") };'''
new = '''    lab_reset();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    // ladder 的 current 必须等于 ENTRY.isolate(FakeQq 的 isolate 指针地址)。
    fake_v8::CURRENT.store(
        fake_v8::FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("adapter") };'''
assert old in s
s = s.replace(old, new)

# d8 arm test: same correction (bootstrap FIRST, then set CURRENT from FAKE_EXPECTED_ISOLATE)
old2 = '''    lab_reset();
    install_fake_v8();
    fake_v8::CURRENT.store(0x1DEA_0001, Ordering::Release);

    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("d8arm") };'''
new2 = '''    lab_reset();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(
        fake_v8::FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );

    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("d8arm") };'''
assert old2 in s
s = s.replace(old2, new2)

# d8_zero test: set CURRENT to H first (bootstrap ok), then to 0 for the refusal
old3 = '''    lab_reset();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("d8zero") };'''
new3 = '''    lab_reset();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(
        fake_v8::FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("d8zero") };'''
assert old3 in s
s = s.replace(old3, new3)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
