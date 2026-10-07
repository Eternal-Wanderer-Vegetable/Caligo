// resident-poll.js — K3-E 排空接收环 + lastSend 状态;返回 JSON。
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var out = JSON.stringify({ sid: R.sid, n: R.recv.length, items: R.recv, lastSend: R.lastSend });
    R.recv = [];
    return out.slice(0, 60000);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
