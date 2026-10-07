import re

# ---- qq_v8.rs: replace include_str constants with D8-adapted literals ----
p = 'crates/caligo-bridge/src/qq_v8.rs'
s = open(p, encoding='utf-8').read()
start = s.find('// —— D8 固定语义脚本')
assert start > 0
new_tail = '''// —— D8 固定语义脚本(js/resident-*.js 的 D8 适配版;K3 现场验证逻辑) ——
// 与 K3 版的差异:①push 记录来源回调(recv/update,D8 去重语义依赖);
// ②msgTime 平台时间捕获;③正文不裁剪(完整正文 D8 验收);④环容量 200 不变。

/// 注册监听器(活会话扫描 + addKernelMsgListener;每会话代次一次)。
pub const D8_START_JS: &str = r"JSSTART
(function () {
  try {
    var G = globalThis;
    if (G.__caligo_res) return 'ALREADY sid=' + G.__caligo_res.sid;
    var q = process._linkedBinding('QQNT');
    var S = q.NodeIQQNTWrapperSession;
    var live = null, sidName = null;
    for (var i = 0; i < 10; i++) {
      try {
        var ls = S.getNTWrapperSession('nt_' + i);
        if (ls && typeof ls === 'object') {
          var sid = ls.getSessionId();
          if (sid && String(sid) !== '0') {
            var ms = ls.getMsgService();
            if (ms && ms.sendMsg) { live = ls; sidName = 'nt_' + i; break; }
          }
        }
      } catch (e) {}
    }
    if (!live) return 'ERR:no live session';
    var ms = live.getMsgService();
    var R = { sid: sidName, session: live, ms: ms, recv: [], seq: 0, listenerId: null };
    var push = function (src, m) {
      try {
        var txt = '';
        var els = m.elements || [];
        for (var j = 0; j < els.length; j++) {
          if (els[j] && els[j].textElement) txt += els[j].textElement.content || '';
        }
        R.recv.push({
          seq: R.seq++, ts: Date.now(), src: src,
          msgId: String(m.msgId || '').slice(0, 40),
          chatType: m.chatType,
          peerUid: String(m.peerUid || '').slice(0, 48),
          peerUin: String(m.peerUin || '').slice(0, 20),
          senderUid: String(m.senderUid || '').slice(0, 48),
          senderUin: String(m.senderUin || '').slice(0, 20),
          msgTime: (typeof m.msgTime === 'number') ? m.msgTime : null,
          sendStatus: m.sendStatus, msgType: m.msgType, subMsgType: m.subMsgType,
          text: txt.slice(0, 8000)
        });
        if (R.recv.length > 200) R.recv.splice(0, R.recv.length - 200);
      } catch (err) {}
    };
    R.listener = {
      onRecvMsg: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push('recv', arr[i]);
        } catch (err) {}
      },
      onMsgInfoListUpdate: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push('update', arr[i]);
        } catch (err) {}
      }
    };
    R.listenerId = ms.addKernelMsgListener(R.listener);
    G.__caligo_res = R;
    return 'ARMED sid=' + sidName + ' lidRet=' + String(R.listenerId).slice(0, 40);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
JSEND";

/// 排空接收环 + 返回 JSON(先出环后返回,出环即从 JS 面消费)。
pub const D8_POLL_JS: &str = r"JSSTART
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var out = JSON.stringify({ sid: R.sid, n: R.recv.length, items: R.recv });
    R.recv = [];
    return out.slice(0, 900000);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
JSEND";

/// 对称移除监听器 + 清理常驻对象。
pub const D8_STOP_JS: &str = r"JSSTART
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var r = 'no-listener';
    try { r = String(R.ms.removeKernelMsgListener(R.listenerId)); } catch (e) { r = 'ERR:' + String(e).slice(0, 100); }
    var sid = R.sid;
    delete G.__caligo_res;
    return 'STOPPED sid=' + sid + ' remove=' + r;
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
JSEND";
'''
s = s[:start] + new_tail
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('qq_v8 d8 scripts embedded')
