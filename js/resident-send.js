// resident-send.js — K3-E 发送(模板):CLI 以 __CALIGO_CHAT__/__CALIGO_PEER__/
// __CALIGO_TEXT__ 占位(JSON 转义后替换)。NapCat 同款四参形态。
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var chat = __CALIGO_CHAT__;
    var peerUid = __CALIGO_PEER__;
    var text = __CALIGO_TEXT__;
    var sv = 0;
    try { sv = R.session.getMSFService().getServerTime(); } catch (e) {}
    var mid = String(R.ms.generateMsgUniqueId(chat, sv));
    var peer = { chatType: chat, guildId: mid, peerUid: peerUid };
    var elems = [{ elementType: 1, textElement: { content: text } }];
    var pr = R.ms.sendMsg('0', peer, elems, new Map());
    R.lastSend = { mid: mid, peerUid: String(peerUid), chat: chat, textLen: text.length, ts: Date.now(), status: 'pending' };
    if (pr && typeof pr.then === 'function') {
      pr.then(function (res) {
        try { R.lastSend.result = JSON.parse(JSON.stringify(res)); R.lastSend.status = 'done'; }
        catch (e) { R.lastSend.status = 'done-raw'; }
      }, function (err) {
        R.lastSend.status = 'rejected';
        try { R.lastSend.error = JSON.stringify(err).slice(0, 500); } catch (e) { R.lastSend.error = String(err).slice(0, 500); }
      });
    }
    return JSON.stringify({ fired: true, mid: mid });
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
