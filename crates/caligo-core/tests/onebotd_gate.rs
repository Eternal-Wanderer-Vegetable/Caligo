//! K4-D0:旧 onebotd runner 默认停用的集成测试。
//!
//! 计划 §7-D0 op3:旧 onebotd 退出并提示旧路线停用;不调用 cli_inject,
//! 不自动加"指定测试实例确认"。本测试以任意参数启动 onebotd,断言立即
//! 以退出码 3 拒绝 —— 全程不 spawn caligo-cli、不触碰任何 QQ 进程。

use std::process::Command;

#[test]
fn onebotd_refuses_legacy_runner_in_default_build() {
    let bin = env!("CARGO_BIN_EXE_onebotd");
    // 旧用法需要的完整参数形态;门控必须先于参数解析拒绝。
    let out = Command::new(bin)
        .args(["--pid", "4294967290", "--env", "deadbeef"])
        .output()
        .expect("spawn onebotd");
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("[K4-D0]"), "stderr={stderr}");
    // 不得出现旧 runner 的启动输出(证明没有进入 ARM/poll 流程)。
    assert!(!stderr.contains("onebotd] pid="));
    assert!(!stdout_has_arm(&out));
}

#[test]
fn onebotd_refuses_even_with_empty_arguments() {
    let bin = env!("CARGO_BIN_EXE_onebotd");
    let out = Command::new(bin).output().expect("spawn onebotd");
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("[K4-D0]"));
}

fn stdout_has_arm(out: &std::process::Output) -> bool {
    String::from_utf8_lossy(&out.stdout).contains("监听已武装")
}
