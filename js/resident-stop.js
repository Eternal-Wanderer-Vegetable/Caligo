// resident-stop.js — K3-E 停止:移除监听 + 清理常驻对象(完整还原)。
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
