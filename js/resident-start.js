// resident-start.js — K3-E 常驻层启动(主 env 执行)。
// 活会话重扫(nt_N 漂移)→ 构造监听 → __caligo_res 常驻对象(环 200)。
// 返回字符串:ARMED sid=... lidRet=... / ALREADY / ERR:...
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
    var R = { sid: sidName, session: live, ms: ms, recv: [], seq: 0, listenerId: null, lastSend: null };
    var push = function (m) {
      try {
        var txt = '';
        var els = m.elements || [];
        for (var j = 0; j < els.length; j++) {
          if (els[j] && els[j].textElement) txt += els[j].textElement.content || '';
        }
        R.recv.push({
          seq: R.seq++, ts: Date.now(),
          msgId: String(m.msgId || '').slice(0, 40),
          chatType: m.chatType,
          peerUid: String(m.peerUid || '').slice(0, 48),
          peerUin: String(m.peerUin || '').slice(0, 20),
          senderUid: String(m.senderUid || '').slice(0, 48),
          senderUin: String(m.senderUin || '').slice(0, 20),
          sendStatus: m.sendStatus, msgType: m.msgType, subMsgType: m.subMsgType,
          text: txt.slice(0, 1200)
        });
        if (R.recv.length > 200) R.recv.splice(0, R.recv.length - 200);
      } catch (err) {}
    };
    R.listener = {
      onRecvMsg: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push(arr[i]);
        } catch (err) {}
      },
      onMsgInfoListUpdate: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push(arr[i]);
        } catch (err) {}
      }
    };
    R.listenerId = ms.addKernelMsgListener(R.listener);
    G.__caligo_res = R;
    return 'ARMED sid=' + sidName + ' lidRet=' + String(R.listenerId).slice(0, 40);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
