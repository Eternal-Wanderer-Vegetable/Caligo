//! K2-03:主 Environment 事件循环点载荷(设计见 local-evidence/k2-03/design.md,
//! 文档 §12;路线由 RequestInterrupt 载荷修订为 uv_async 载荷)。
//!
//! 链条(全部离线解码 + 页校验):env+0xB0 → IsolateData → +0x11E8 → uv_loop_t*。
//! - mode 0:干跑(解析/校验/uv_loop_alive,不 init);
//! - mode 1:uv_async_init + uv_async_send,回调只做原子写(轮转点证明);
//! - mode 2:uv_async 回调内上下文阶梯(isolate 一致性 → entered → incumbent)。
//!   实测(mode 2 第一轮):uv 轮转点上两个访问器皆空(JS 空闲时上下文未 entered),
//!   安全门按设计拒绝执行——mode 2 保留为证据轮。
//! - mode 3(当前主路线):**中断点捕获 + 循环点执行**。RequestInterrupt 只在
//!   v8 执行 JS 的间隙被处理(此时上下文必然 entered),其回调仅读取 entered
//!   Local 槽内的 tagged Context 指针(纯原子写);随后 uv_async 回调在
//!   HandleScope 内经 `HandleScope::CreateHandle`(ord 1066,已导出)把 tagged
//!   指针转为合法 Local<Context>,在其上 Script::Compile/Run 只读枚举脚本。
//!
//! 纪律(计划 §3.1):回调内零 IO、零堆分配、零锁——静态原子 + 预分配缓冲。
//! uv_async 句柄不 close(需与 loop 同步),随进程退出回收,如实记录。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::envrun::append_stage;

/// 执行结果码(caligo_async_run 的返回值)。
pub mod async_code {
    /// 流程完成(各阶段成败以报告为准)。
    pub const OK: u32 = 0;
    /// ctx 或报告路径指针无效。
    pub const ERR_NULL_PATH: u32 = 1;
    /// 报告路径不是合法 UTF-16。
    pub const ERR_BAD_PATH: u32 = 2;
    /// env 候选页不可读 / 布局链断裂(拒绝继续)。
    pub const ERR_ENV_INVALID: u32 = 3;
    /// mode 非法(0/1/2/3 之外)。
    pub const ERR_BAD_MODE: u32 = 4;
    /// uv 阶段失败(alive=false / init 失败)。
    pub const ERR_UV: u32 = 5;
    /// mode 3 phase A:中断点未捕获到 entered 上下文(JS 间隙未出现/上下文空)。
    pub const ERR_NO_CAPTURE: u32 = 6;
}

/// mode 2 脚本选择(ctx.script):0 = 只读指纹(v2),1 = load 探针(v3,K2-04)。
pub const SCRIPT_FINGERPRINT: u32 = 0;
pub const SCRIPT_LOAD_PROBE: u32 = 1;
pub const SCRIPT_LOAD_HARVEST: u32 = 2;
pub const SCRIPT_GLOBAL_INTROSPECT: u32 = 3;
pub const SCRIPT_MAINMODULE_PROBE: u32 = 4;
pub const SCRIPT_ELECTRON_ENUM: u32 = 5;
pub const SCRIPT_RENDERER_BRIDGE: u32 = 6;
pub const SCRIPT_API_MAP: u32 = 7;
pub const SCRIPT_WEBPACK_MAP: u32 = 8;
pub const SCRIPT_IPCMAIN_MAP: u32 = 9;
pub const SCRIPT_HANDLER_TEXT: u32 = 10;
pub const SCRIPT_RM_TAP: u32 = 11;
pub const SCRIPT_RM_TAP_REMOVE: u32 = 12;
pub const SCRIPT_RM_TAP_FILTERED: u32 = 13;
pub const SCRIPT_INVOKE_HANDLERS: u32 = 14;
pub const SCRIPT_RM_TAP_V3: u32 = 15;
pub const SCRIPT_RENDERER_DEEP: u32 = 16;
pub const SCRIPT_IPCIMPL_TEXT: u32 = 17;
pub const SCRIPT_PROCESS_TOPO: u32 = 18;
pub const SCRIPT_MODULE_LOADLIST: u32 = 19;
pub const SCRIPT_K3_C0: u32 = 20;
pub const SCRIPT_K3_C1: u32 = 21;
pub const SCRIPT_K3_DIAG: u32 = 22;
pub const SCRIPT_K3_UID: u32 = 23;
pub const SCRIPT_K3_PROBE_ROUTE: u32 = 24;
pub const SCRIPT_K3_SEND: u32 = 25;
pub const SCRIPT_K3_DOM: u32 = 26;
pub const SCRIPT_K3_DOM_INJECT: u32 = 27;
pub const SCRIPT_K3_DOM_INJECT2: u32 = 28;
pub const SCRIPT_K3_DOM_INJECT3: u32 = 29;
pub const SCRIPT_K3_DOM_SEND: u32 = 30;
pub const SCRIPT_K3_SENDCHK: u32 = 31;
pub const SCRIPT_K3_SENDCHK2: u32 = 32;
pub const SCRIPT_K3_ENTER: u32 = 33;
pub const SCRIPT_K3_ENTERCHK: u32 = 34;
pub const SCRIPT_K3_SENDINPUT: u32 = 35;
pub const SCRIPT_K3_SI2: u32 = 36;
pub const SCRIPT_K3_WINSTATE: u32 = 37;
pub const SCRIPT_K3_RESTORE_SEND: u32 = 38;
pub const SCRIPT_K3_RS2: u32 = 39;
pub const SCRIPT_K3_HEADLESS_RESP: u32 = 40;
pub const SCRIPT_K3_IPCIMPL_ON: u32 = 41;
pub const SCRIPT_K3_WCSEND_ARM: u32 = 42;
pub const SCRIPT_K3_WCSEND_READ: u32 = 43;
pub const SCRIPT_K3_WCSEND_RESTORE: u32 = 44;
pub const SCRIPT_K3_QQNT_KEYS: u32 = 45;
pub const SCRIPT_K3_PROTO: u32 = 46;
pub const SCRIPT_K3_SESSION_TEST: u32 = 47;
pub const SCRIPT_K3_INIT_DISC: u32 = 48;
pub const SCRIPT_K3_LIVE_SESSION: u32 = 49;
pub const SCRIPT_K3_ENGINE_LIVE: u32 = 50;
pub const SCRIPT_K3_DEVINFO: u32 = 51;
pub const SCRIPT_K3_UIDKEY: u32 = 52;
pub const SCRIPT_K3_INITLADDER: u32 = 53;
pub const SCRIPT_K3_STARTUP: u32 = 54;
pub const SCRIPT_K3_SESSIONID: u32 = 55;
pub const SCRIPT_K3_IDSHAPE: u32 = 56;
pub const SCRIPT_K3_LIVE2: u32 = 57;
pub const SCRIPT_K3_STATICS_SWEEP: u32 = 58;
pub const SCRIPT_K3_USERDATA: u32 = 59;
pub const SCRIPT_K3_ENGINELADDER: u32 = 60;
pub const SCRIPT_K3_ENG2: u32 = 61;
pub const SCRIPT_K3_CIDCMD: u32 = 62;
pub const SCRIPT_K3_FOCUS_ENTER: u32 = 63;
pub const SCRIPT_K3_SNOW_TAP: u32 = 64;
pub const SCRIPT_K3_GROUPLIST2: u32 = 65;
pub const SCRIPT_K3_DIRECT_HANDLER: u32 = 66;
pub const SCRIPT_K3_DH2: u32 = 67;
pub const SCRIPT_K3_SEND2: u32 = 68;
pub const SCRIPT_K3_SENDLADDER: u32 = 69;
pub const SCRIPT_K3_SEND3: u32 = 70;
pub const SCRIPT_K3_ARITY: u32 = 71;
pub const SCRIPT_K3_SEND5: u32 = 72;
pub const SCRIPT_K3_PROTOARITY: u32 = 73;
pub const SCRIPT_K3_NAPSEND: u32 = 74;
pub const SCRIPT_K3_ONLINEDEV: u32 = 75;
pub const SCRIPT_K3_SESSIONKEY2: u32 = 76;
pub const SCRIPT_K3_LIVESVC: u32 = 77;
pub const SCRIPT_K3_SESSIONSCAN: u32 = 78;
pub const SCRIPT_K3_DIRECTSEND: u32 = 79;
pub const SCRIPT_K3_DSREAD: u32 = 80;
pub const SCRIPT_K3_RECV_ARM: u32 = 81;
pub const SCRIPT_K3_RECV_ARM2: u32 = 82;
pub const SCRIPT_K3_C2CSEND: u32 = 83;

/// K3-C2(设计 local-evidence/k3-c/design.md;pivot 至群聊样本,test-scope §3):
/// 经 ipcImpl.send(原生总线,与聊天同路)发一条群消息到 GROUP-C。
/// 正文 CALIGO-K3-GROUP-001(唯一正文);只发一次;错误经 LogApi tap 观测。
pub const K3_SEND_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r27'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(!st){var probe=\"(function(){var W=window;var o={};try{var cmd={cmdName:'nodeIKernelMsgService/sendMsg',cmdType:'invoke',payload:[0,{chatType:2,peerUid:'263402786',guildId:''},[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]]};W.ipcImpl.send('ntApi',cmd);o.sent=true}catch(e){o.err=String(e).slice(0,300)}return JSON.stringify(o)})()\";var out=null;target.executeJavaScript(probe,false).then(function(r){out='FIRE:'+String(r).slice(0,300)},function(e){out='REJECT:'+String(e).slice(0,200)});var t0=Date.now();while(out===null&&Date.now()-t0<1500){}G['__caligo_r27']=out||'FIRE-ACCEPTED(async)';return G['__caligo_r27']}return 'ERR:bad state '+String(st).slice(0,40)}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 路由验证:发一个不存在的方法(若管线通会产生错误日志)+ 真实 getUidByUin。
pub const K3_PROBE_ROUTE_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r25'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(!st){var probe=\"(function(){var W=window;var o={};try{W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelXxxService/caligoNoSuchMethodProbe',cmdType:'invoke',payload:[1,2,3]});o.p1='sent'}catch(e){o.p1Err=String(e).slice(0,200)}try{W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]});o.p2='sent'}catch(e){o.p2Err=String(e).slice(0,200)}return JSON.stringify(o)})()\";var out=null;target.executeJavaScript(probe,false).then(function(r){out=String(r).slice(0,300)},function(e){out='REJECT:'+String(e).slice(0,200)});var t0=Date.now();while(out===null&&Date.now()-t0<3000){}G['__caligo_r25']=out||'TIMEOUT';return G['__caligo_r25']}return 'ERR:bad state '+String(st).slice(0,40)}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-C1-v4:ipcImpl.send 为 Promise 风格(contextBridge 不可传函数)。
/// 无函数参数直调 getUidByUin[3089665724],Promise.then 捕获响应。
pub const K3_UID_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r24'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r24']='READ_DONE:'+String(G['__caligo_r24_tmp']||'').slice(0,4000);return G['__caligo_r24']}if(!st){var probe=\"(function(){var W=window;var p=W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]});if(p&&typeof p.then==='function'){p.then(function(r){W.__caligo_uid='RES:'+JSON.stringify(r).slice(0,3000)},function(e){W.__caligo_uid='ERRP:'+String(e).slice(0,400)});return 'promise-armed'}return 'NOT-PROMISE:'+typeof p})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r24_tmp']='FIRE:'+String(r).slice(0,200)},function(e){G['__caligo_r24_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r24']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 诊断:fire 探针的 evaluate 结果直接写主 env(不丢 rejection)。
pub const K3_DIAG_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r23'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r23']='READ_DONE:'+String(G['__caligo_r23_tmp']||'').slice(0,1000);return G['__caligo_r23']}if(!st){var probe=\"(function(){var W=window;var o={};try{o.dtcType=typeof W.dtResponseCallbacks;o.ipcImplType=typeof W.ipcImpl;o.ipcRendererType=typeof W.ipcRenderer}catch(e){o.accErr=String(e).slice(0,100)}try{W.__caligo_diag=7;o.diagSet=W.__caligo_diag}catch(e){o.diagErr=String(e).slice(0,100)}try{W.dtResponseCallbacks['diag-1']=function(){};o.dtcSet=Object.getOwnPropertyNames(W.dtResponseCallbacks).length;delete W.dtResponseCallbacks['diag-1']}catch(e){o.dtcErr=String(e).slice(0,150)}try{W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]},function(){});o.expB='called'}catch(e){o.expBErr=String(e).slice(0,400)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r23_tmp']='OK:'+String(r).slice(0,900)},function(e){G['__caligo_r23_tmp']='REJECT:'+String(e).slice(0,400)});G['__caligo_r23']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-C1(设计 local-evidence/k3-c/design.md 阶段 C-0 修订):
/// 实验 A:自注册 dtResponseCallbacks[cid] + 4 参合成信封(frame,request,cmd);
/// 实验 B:ipcImpl.send 签名试探('ntApi', cmd, cb)。响应进 __caligo_c1_resp。
pub const K3_C1_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r22'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){var p2=\"(function(){var W=window;return JSON.stringify({resp:W.__caligo_c1_resp||[],n:(W.__caligo_c1_resp||[]).length,dtc:W.dtResponseCallbacks?Object.getOwnPropertyNames(W.dtResponseCallbacks).length:-1})})()\";target.executeJavaScript(p2,false).then(function(r){G['__caligo_r22']=String(r).slice(0,64000)},function(e){G['__caligo_r22']='ERR:'+String(e).slice(0,300)});return 'kicked-read'}if(st&&st.indexOf&&st.indexOf('{')===0){G['__caligo_r22']='waiting';return st}if(!st){var probe=\"(function(){var W=window;var o={ok:true};try{W.__caligo_c1_resp=[];var rec=function(tag){return function(){try{var a=Array.prototype.slice.call(arguments);var e={tag:tag,args:[]};for(var i=0;i<a.length;i++){var v=a[i];var t=typeof v;e.args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,4096):String(v).slice(0,512)})}W.__caligo_c1_resp.push(e)}catch(err){}}};var cid='c1-'+((W.crypto&&W.crypto.randomUUID)?W.crypto.randomUUID():Date.now());try{W.dtResponseCallbacks[cid]=rec('dtcA');o.dtcSet=true}catch(e){o.dtcErr=String(e).slice(0,120)}try{var frame={type:'frame',sender:{},senderFrame:{},frameId:1,processId:5,frameTreeNodeId:2};W.ipcRenderer.send('RM_IPCFROM_RENDERER2',frame,{type:'request',callbackId:cid,eventName:'ntApi',peerId:2},{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]});o.expA='sent4'}catch(e){o.expAErr=String(e).slice(0,150)}try{var r2=W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]},rec('ipcImplB'));o.expB=typeof r2}catch(e){o.expBErr=String(e).slice(0,300)}W.__caligo_c1_resp.push({tag:'fire',o:o});return JSON.stringify(o)})()\";target.executeJavaScript(probe,false);G['__caligo_r22']='waiting';return 'kicked'}return 'ERR:bad state '+String(st).slice(0,40)}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";
/// K3-C0(设计 local-evidence/k3-c/design.md):renderer 内订阅候选响应通道 +
/// 合成信封重放(①AvatarService 只读回放 ②getUidByUin[3089665724])。
/// 两段式:主 env `__caligo_r20`;renderer 侧 `__caligo_c0_resp` 存响应。
pub const K3_C0_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r20'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){var probe2=\"(function(){var W=window;return JSON.stringify({resp:W.__caligo_c0_resp||[],n:(W.__caligo_c0_resp||[]).length})})()\";target.executeJavaScript(probe2,false).then(function(r){G['__caligo_r20']=String(r).slice(0,64000)},function(e){G['__caligo_r20']='ERR:'+String(e).slice(0,300)});return 'kicked-read'}if(st&&st.indexOf&&st.indexOf('{')===0){G['__caligo_r20']='waiting';return st}if(!st){var probe=\"(function(){var W=window;try{if(!W.__caligo_c0_armed){W.__caligo_c0_resp=[];var chans=['RM_IPCTORENDERER','RM_IPCTORENDERER2','RM_IPCFROM_MAIN','RM_RESPONSE','RM_IPCRESPONSE','ntApiResponse','RM_IPCFROM_RENDERER2'];W.__caligo_c0_subs=[];var rec=function(ch){return function(){try{var a=Array.prototype.slice.call(arguments);var e={ch:ch,args:[]};for(var i=0;i<a.length;i++){var v=a[i];var t=typeof v;try{e.args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,4096):String(v).slice(0,512)})}catch(err){e.args.push({t:t,j:'ERR'})}}}W.__caligo_c0_resp.push(e);if(W.__caligo_c0_resp.length>20){W.__caligo_c0_resp.splice(0,W.__caligo_c0_resp.length-20)}}catch(err){}}};for(var i=0;i<chans.length;i++){try{var f=rec(chans[i]);W.ipcRenderer.on(chans[i],f);W.__caligo_c0_subs.push({ch:chans[i],fn:f})}catch(err){}}W.__caligo_c0_armed=true}var uid='u_enHd8F-nTKH4WSVV_hF3Kg';var mk=function(cid){return {type:'request',callbackId:cid,eventName:'ntApi',peerId:2}};var cid1=(W.crypto&&W.crypto.randomUUID)?W.crypto.randomUUID():'c0a-'+Date.now();W.ipcRenderer.send('RM_IPCFROM_RENDERER2',mk(cid1),{cmdName:'nodeIKernelAvatarService/getMembersAvatarPath',cmdType:'invoke',payload:[{uids:[uid],clarity:0}]});var cid2=(W.crypto&&W.crypto.randomUUID)?W.crypto.randomUUID():'c0b-'+Date.now();W.ipcRenderer.send('RM_IPCFROM_RENDERER2',mk(cid2),{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]});return JSON.stringify({fired:true,cid1:cid1,cid2:cid2,subs:(W.__caligo_c0_subs||[]).length})})()\";var r1=target.executeJavaScript(probe,false);G['__caligo_r20']='waiting';return 'kicked'}return 'ERR:bad state '+String(st).slice(0,40)}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";


/// mode 2 的枚举脚本:只读(不调用任何 QQ 函数)、自包含、异常全捕获。
/// 第二版:除顶层键外,另取 load 的类型/源码指纹、process.versions、
/// require 可用性与 globalThis 键表(截断),全部为纯读取。
pub const ENUM_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true,keys:Object.getOwnPropertyNames(m),loadType:typeof m.load};try{o.loadSrc=String(m.load).slice(0,300)}catch(e){o.loadSrcErr=String(e)}try{o.versions=process.versions}catch(e){}try{o.hasRequire=typeof require;o.hasProcess=typeof process}catch(e){}try{o.globalKeys=Object.getOwnPropertyNames(globalThis).slice(0,120)}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-04 load 探针(K2-04 设计记录):纯读描述符/name/length → **无参调用一次
/// load()** → 记录返回值形态或异常消息。不传参、不链式调用返回值成员、不赋值。
pub const LOAD_PROBE_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true};var d=Object.getOwnPropertyDescriptor(m,'load');o.desc=d&&{writable:d.writable,enumerable:d.enumerable,configurable:d.configurable,hasGet:!!d.get,hasSet:!!d.set};o.fnName=m.load.name;o.fnArity=m.load.length;try{var r=m.load();o.retType=typeof r;if(r&&(typeof r==='object'||typeof r==='function')){o.retKeys=Object.getOwnPropertyNames(r).slice(0,200);try{o.retCtor=r.constructor&&r.constructor.name}catch(e){}}else{o.retPrim=String(r).slice(0,100)}}catch(e){o.callErr=String(e)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-04 v4 探针:错误收割(load 各位参喂错误类型,校验错误暴露签名)+
/// 全量 globalThis 键表。不触发任何模块执行。
pub const LOAD_HARVEST_SCRIPT: &str = "(function(){try{var m=process._linkedBinding('major');var o={ok:true};var probes=[['load(1)',function(){return m.load(1)}],['load(x)',function(){return m.load('x')}],['load(x,y)',function(){return m.load('x','y')}],['load(x,y,z)',function(){return m.load('x','y','z')}]];o.harvest=[];for(var i=0;i<probes.length;i++){var e={name:probes[i][0]};try{var r=probes[i][1]();e.retType=typeof r;if(r&&typeof r==='object'){e.retKeys=Object.getOwnPropertyNames(r).slice(0,80)}}catch(err){e.err=String(err).slice(0,300)}o.harvest.push(e)}try{var g=Object.getOwnPropertyNames(globalThis);o.globalCount=g.length;o.globalKeys=g}catch(e){o.globalErr=String(e)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-05 v5 探针:QQ 全局结构(键名+类型,不取值)+ launcher 二级展开 +
/// globalThis/process 的 Symbol 键(服务容器常藏于 Symbol 键后)。
pub const GLOBAL_INTROSPECT_SCRIPT: &str = "(function(){try{var o={ok:true};var names=['launcher','TIMES','TIMES_LABEL','loginWin','loginWindowPid','authData','statusData','qqLocked','startSequence','isFirstWinFromPool','multiInstancePort','isAppQuitting','quitAppDirectly','scWindowPid','isGlobalDark','shortCutDataMap','hiddenPoolWindowPid','localEmojiConfigUpdated'];o.targets={};function shape(v,depth){var t=typeof v;var e={type:t};try{if(v===null){e.type='null'}else if(t==='object'||t==='function'){e.keys=Object.getOwnPropertyNames(v).slice(0,120);if(t==='function'){e.fnArity=v.length}if(depth>0){e.children={};var ks=e.keys;for(var i=0;i<ks.length&&i<40;i++){var cv;try{cv=v[ks[i]]}catch(err){continue}var ct=typeof cv;if(ct==='object'&&cv!==null){e.children[ks[i]]={type:ct,keys:Object.getOwnPropertyNames(cv).slice(0,60)}}}}}}catch(err){e.err=String(err).slice(0,150)}return e}for(var i=0;i<names.length;i++){var n=names[i];try{o.targets[n]=shape(globalThis[n],n==='launcher'?1:0)}catch(err){o.targets[n]={err:String(err).slice(0,150)}}}try{var syms=Object.getOwnPropertySymbols(globalThis);o.globalSymbols=[];for(var j=0;j<syms.length;j++){var s={desc:syms[j].description||String(syms[j])};try{var sv=globalThis[syms[j]];s.type=typeof sv;if(sv&&typeof sv==='object'){s.keys=Object.getOwnPropertyNames(sv).slice(0,60)}}catch(err){}o.globalSymbols.push(s)}}catch(err){o.symbolErr=String(err)}try{var psyms=Object.getOwnPropertySymbols(process);o.processSymbols=[];for(var k=0;k<psyms.length;k++){o.processSymbols.push(psyms[k].description||String(psyms[k]))}}catch(err){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-05 v6 探针:process.mainModule(CJS 主模块把手)+ process 自有键表 +
/// require.cache 键名(模块清单)。仍纯读:不调用 require。
pub const MAINMODULE_PROBE_SCRIPT: &str = "(function(){try{var o={ok:true};try{var mm=process.mainModule;o.mainModuleExists=mm!==undefined&&mm!==null;if(mm){o.mmKeys=Object.getOwnPropertyNames(mm).slice(0,80);o.mmFilename=String(mm.filename||'').slice(0,300);o.mmRequireType=typeof mm.require;o.mmExportsKeys=(mm.exports&&typeof mm.exports==='object')?Object.getOwnPropertyNames(mm.exports).slice(0,100):undefined;o.mmCtor=mm.constructor&&mm.constructor.name}}catch(e){o.mmErr=String(e).slice(0,200)}try{o.processKeys=Object.getOwnPropertyNames(process).slice(0,250)}catch(e){o.pErr=String(e)}try{if(process.mainModule&&process.mainModule.require){var cache=process.mainModule.require.cache;o.cacheCount=Object.keys(cache||{}).length;var ck=Object.getOwnPropertyNames(cache||{});o.cachePaths=ck.slice(0,150);o.cacheTotal=ck.length}}catch(e){o.cacheErr=String(e).slice(0,200)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-06 v7 探针:require('electron')(内置缓存命中)→ 模块键名 +
/// webContents 枚举(id/type/url/destroyed,纯查询)。不触碰 executeJavaScript。
pub const ELECTRON_ENUM_SCRIPT: &str = "(function(){try{var o={ok:true};var req=process.mainModule&&process.mainModule.require;o.requireType=typeof req;if(typeof req!=='function'){o.err='no require';return JSON.stringify(o)}var electron=req('electron');o.electronKeys=Object.getOwnPropertyNames(electron).slice(0,150);try{var wc=electron.webContents.getAllWebContents();o.count=wc.length;o.list=[];for(var i=0;i<wc.length;i++){var c=wc[i];var e={idx:i};try{e.id=c.getId()}catch(err){}try{e.type=c.getType()}catch(err){}try{e.url=String(c.getURL()).slice(0,200)}catch(err){}try{e.destroyed=c.isDestroyed()}catch(err){}o.list.push(e)}}catch(err){o.wcErr=String(err).slice(0,200)}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-07 两段式探针(设计 local-evidence/k2-07/design.md):
/// 段 A(URL 含 #/main/message 的 webContents 上 executeJavaScript 只读探针,
/// 结果挂 `__caligo_r7`);段 B(读回并删除)。renderer 探针纯键名/typeof。
pub const RENDERER_BRIDGE_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r7'];if(st!==undefined&&st!=='waiting'){delete G['__caligo_r7'];return String(st).slice(0,60000)}if(st==='waiting'){return 'waiting'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){var o={ok:true};try{o.href=String(location.href).slice(0,150)}catch(e){}try{var wk=Object.getOwnPropertyNames(window);o.windowKeysTotal=wk.length;o.windowKeysTail=wk.slice(-160)}catch(e){}try{o.hasProcess=typeof process;o.hasRequire=typeof require}catch(e){}var sus=['ntApi','qq','qqnt','ipc','bridge','services','windowApi'];o.suspects={};for(var i=0;i<sus.length;i++){try{o.suspects[sus[i]]=typeof window[sus[i]]}catch(e){}}return JSON.stringify(o)})()\";G['__caligo_r7']='waiting';target.executeJavaScript(probe,false).then(function(r){G['__caligo_r7']=String(r).slice(0,60000)},function(e){G['__caligo_r7']='ERR:'+String(e).slice(0,300)});return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-08 两段式探针(`__caligo_r8`):preloadApi / experimentalAPIs 结构地图
/// (键名+typeof+arity+一层子键;ipcRenderer 仅键名)。零调用、零属性值。
pub const API_MAP_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r8'];if(st!==undefined&&st!=='waiting'){delete G['__caligo_r8'];return String(st).slice(0,64000)}if(st==='waiting'){return 'waiting'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){function shape(v,topCap,childCap,childN){var e={};try{e.type=typeof v;if(v===null){e.type='null';return e}if(typeof v!=='object'&&typeof v!=='function'){return e}var ks=Object.getOwnPropertyNames(v);e.total=ks.length;e.keys=ks.slice(0,topCap);e.types={};for(var i=0;i<e.keys.length;i++){var t=typeof v[e.keys[i]];e.types[e.keys[i]]=t;if(t==='function'){try{e.types[e.keys[i]]='function('+v[e.keys[i]].length+')'}catch(err){}}}if(childN>0){e.children={};var n=0;for(var j=0;j<e.keys.length&&n<childN;j++){var cv;try{cv=v[e.keys[j]]}catch(err){continue}if(cv&&typeof cv==='object'){e.children[e.keys[j]]=Object.getOwnPropertyNames(cv).slice(0,childCap);n++}}}}catch(err){e.err=String(err).slice(0,150)}return e}var o={ok:true};try{o.preloadApi=shape(window.preloadApi,200,80,60)}catch(e){o.paErr=String(e).slice(0,150)}try{o.experimentalAPIs=shape(window.experimentalAPIs,120,60,40)}catch(e){o.eaErr=String(e).slice(0,150)}try{o.ipcRendererType=typeof window.ipcRenderer;o.ipcRendererKeys=window.ipcRenderer?Object.getOwnPropertyNames(window.ipcRenderer).slice(0,60):undefined}catch(e){}return JSON.stringify(o)})()\";G['__caligo_r8']='waiting';target.executeJavaScript(probe,false).then(function(r){G['__caligo_r8']=String(r).slice(0,64000)},function(e){G['__caligo_r8']='ERR:'+String(e).slice(0,300)});return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-09 两段式探针(`__caligo_r9`):webpack require 捕获(唯一假块 id,空模块表)
/// + 工厂表 toString 文本搜索(nodeIKernel*/getService 标记;不调用任何工厂)。
pub const WEBPACK_MAP_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r9'];if(st!==undefined&&st!=='waiting'){delete G['__caligo_r9'];return String(st).slice(0,64000)}if(st==='waiting'){return 'waiting'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){var o={ok:true};try{var d=window.dtResponseCallbacks;if(d&&typeof d==='object'){var dk=Object.getOwnPropertyNames(d);o.dtcTotal=dk.length;o.dtcKeys=dk.slice(0,20)}}catch(e){o.dtcErr=String(e).slice(0,150)}try{var W=window.webpackChunkqq_chat;if(!W||!W.push){o.wpErr='no webpackChunkqq_chat';return JSON.stringify(o)}var captured=null;var id='caligo-'+Math.random().toString(36).slice(2);W.push([[id],{},function(req){captured=req}]);if(!captured){o.wpErr='capture failed';return JSON.stringify(o)}o.captured=true;var mods=captured.m;var ids=Object.keys(mods);o.moduleCount=ids.length;var marks=['nodeIKernel','getMsgService','getSessionService','getBuddyService','getGroupService','getLoginService','getService'];o.hits={};o.hitCount=0;for(var i=0;i<ids.length&&o.hitCount<40;i++){var src='';try{src=Function.prototype.toString.call(mods[ids[i]])}catch(e){continue}var found=[];for(var m=0;m<marks.length;m++){if(src.indexOf(marks[m])>=0){found.push(marks[m])}}if(found.length){var pos=src.indexOf(found[0]);o.hits[ids[i]]={marks:found.slice(0,5),snip:src.slice(Math.max(0,pos-40),pos+160)};o.hitCount++}}}catch(e){o.wpErr=String(e).slice(0,200)}return JSON.stringify(o)})()\";G['__caligo_r9']='waiting';target.executeJavaScript(probe,false).then(function(r){G['__caligo_r9']=String(r).slice(0,64000)},function(e){G['__caligo_r9']='ERR:'+String(e).slice(0,300)});return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-09 第二轮(主 env):ipcMain._events 通道名枚举(所有注册的 IPC 通道)。
pub const IPCMAIN_MAP_SCRIPT: &str = "(function(){try{var o={ok:true};var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return JSON.stringify({ok:false,error:'no require'})}var electron=req('electron');try{var im=electron.ipcMain;var ev=im._events||{};var k=Object.getOwnPropertyNames(ev);o.channelCount=k.length;o.channels=k.slice(0,400)}catch(e){o.imErr=String(e).slice(0,200)}try{var ks=Object.getOwnPropertySymbols(electron.ipcMain);o.imSymbols=[];for(var i=0;i<ks.length;i++){o.imSymbols.push(String(ks[i].description||ks[i]))}}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-09 第三轮(主 env):RM_IPCFROM_RENDERER* 中继 handler 的 toString 文本
/// 搜索服务标记 + process._events 键名。只读;字节码占位符如实记录。
pub const HANDLER_TEXT_SCRIPT: &str = "(function(){try{var o={ok:true};var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return JSON.stringify({ok:false,error:'no require'})}var electron=req('electron');var ev=electron.ipcMain._events||{};var chans=['RM_IPCFROM_RENDERER2','RM_IPCFROM_RENDERER4','RM_IPCFROM_RENDERER5','RM_IPCFROM_RENDERER6','RM_IPCFROM_RENDERER7'];o.handlers={};var marks=['nodeIKernel','getMsgService','getSessionService','getBuddyService','getGroupService','getLoginService','getService','invoke'];for(var c=0;c<chans.length;c++){var ch=chans[c];var h=ev[ch];if(!h){o.handlers[ch]={missing:true};continue}var arr=Array.isArray(h)?h:[h];var e={listeners:arr.length,texts:[]};for(var i=0;i<arr.length&&i<3;i++){var t='';try{t=Function.prototype.toString.call(arr[i])}catch(err){t='ERR:'+String(err).slice(0,80)}var info={len:t.length,placeholder:t.indexOf('[native code]')>=0};var found=[];for(var m=0;m<marks.length;m++){if(t.indexOf(marks[m])>=0){found.push(marks[m])}}if(found.length){var pos=t.indexOf(found[0]);info.marks=found.slice(0,6);info.snip=t.slice(Math.max(0,pos-60),pos+240)}e.texts.push(info)}o.handlers[ch]=e}try{var pe=process._events||{};var pk=Object.getOwnPropertyNames(pe);o.processEvents=pk.slice(0,80);o.processEventCount=pk.length}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-10 加性 RM tap(设计 local-evidence/k2-10/design.md;执行者已认可):
/// 状态机 `__caligo_r10`。段 A 安装 5 通道旁听器(全 try/catch,环形缓冲 50×8KB,
/// 记录器引用存 `__caligo_tap_fns` 可拆卸);段 B 取走缓冲(读后清空,继续观察)。
pub const RM_TAP_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r10'];if(st==='waiting'){var buf=G['__caligo_tap']||[];if(buf.length===0){return 'empty'}var out=JSON.stringify({n:buf.length,items:buf});G['__caligo_tap']=[];return out.slice(0,64000)}if(st!==undefined){delete G['__caligo_r10'];return 'ERR:bad state'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');if(G['__caligo_tap_fns']){return 'ERR:already installed'}var chans=['RM_IPCFROM_RENDERER2','RM_IPCFROM_RENDERER4','RM_IPCFROM_RENDERER5','RM_IPCFROM_RENDERER6','RM_IPCFROM_RENDERER7'];G['__caligo_tap']=[];var fns=[];var rec=function(ch){return function(){try{var G2=globalThis;var b=G2['__caligo_tap'];if(!b){return}var e={ch:ch,t:Date.now(),argc:arguments.length,args:[]};for(var i=0;i<arguments.length;i++){var a=arguments[i];var t=typeof a;if(t==='object'&&a!==null){try{e.args.push({t:t,j:JSON.stringify(a).slice(0,8192)})}catch(err){e.args.push({t:t,j:'ERR:'+String(err).slice(0,80)})}}else{e.args.push({t:t,s:String(a).slice(0,512)})}}b.push(e);if(b.length>50){b.splice(0,b.length-50)}}catch(err){}}};for(var i=0;i<chans.length;i++){var f=rec(chans[i]);fns.push({ch:chans[i],fn:f});electron.ipcMain.on(chans[i],f)}G['__caligo_tap_fns']=fns;G['__caligo_r10']='waiting';return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-10 tap 移除:removeListener 全部记录器 + 删除状态/缓冲全局,回移除计数。
pub const RM_TAP_REMOVE_SCRIPT: &str = "(function(){try{var G=globalThis;var fns=G['__caligo_tap_fns'];var n=0;if(fns){var req=process.mainModule&&process.mainModule.require;var electron=req('electron');for(var i=0;i<fns.length;i++){try{electron.ipcMain.removeListener(fns[i].ch,fns[i].fn);n++}catch(err){}}}delete G['__caligo_tap_fns'];delete G['__caligo_tap'];delete G['__caligo_r10'];return JSON.stringify({removed:n})}catch(e){return JSON.stringify({removed:-1,error:String(e).slice(0,200)})}})()";

/// K2-10 tap v2(过滤版):先移除旧记录器,再装"跳过 LogApi(cmdName==='info')"
/// 的记录器——业务调用不再被日志洪流挤出环形缓冲。引用仍存 `__caligo_tap_fns`
/// (script=12 移除逻辑通用)。
pub const RM_TAP_FILTERED_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var old=G['__caligo_tap_fns'];var removed=0;if(old){for(var i=0;i<old.length;i++){try{electron.ipcMain.removeListener(old[i].ch,old[i].fn);removed++}catch(err){}}}var chans=['RM_IPCFROM_RENDERER2','RM_IPCFROM_RENDERER4','RM_IPCFROM_RENDERER5','RM_IPCFROM_RENDERER6','RM_IPCFROM_RENDERER7'];G['__caligo_tap']=[];var fns=[];var rec=function(ch){return function(){try{var b=G['__caligo_tap'];if(!b){return}var a2=arguments[2];try{if(a2&&typeof a2==='object'&&a2.cmdName==='info'){return}}catch(err){}var e={ch:ch,t:Date.now(),argc:arguments.length,args:[]};for(var i=0;i<arguments.length;i++){var a=arguments[i];var t=typeof a;if(t==='object'&&a!==null){try{e.args.push({t:t,j:JSON.stringify(a).slice(0,8192)})}catch(err){e.args.push({t:t,j:'ERR:'+String(err).slice(0,80)})}}else{e.args.push({t:t,s:String(a).slice(0,512)})}}b.push(e);if(b.length>50){b.splice(0,b.length-50)}}catch(err){}}};for(var i=0;i<chans.length;i++){var f=rec(chans[i]);fns.push({ch:chans[i],fn:f});electron.ipcMain.on(chans[i],f)}G['__caligo_tap_fns']=fns;G['__caligo_r10']='waiting';return JSON.stringify({kicked:true,removed:removed,installed:fns.length})}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-10 追加:ipcMain._invokeHandlers 键名(invoke/handle 体系,区别于 _events)
/// + _events 复查。只读。
pub const INVOKE_HANDLERS_SCRIPT: &str = "(function(){try{var o={ok:true};var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return JSON.stringify({ok:false,error:'no require'})}var electron=req('electron');try{var ih=electron.ipcMain._invokeHandlers||{};var k=Object.getOwnPropertyNames(ih);o.invokeCount=k.length;o.invokeChannels=k.slice(0,500)}catch(e){o.ihErr=String(e).slice(0,200)}try{var ev=electron.ipcMain._events||{};o.eventChannels=Object.getOwnPropertyNames(ev).slice(0,200)}catch(e){}try{var syms=Object.getOwnPropertySymbols(electron.ipcMain);o.symbols=[];for(var i=0;i<syms.length;i++){o.symbols.push(String(syms[i].description||syms[i]))}}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-10 tap v3:过滤 LogApi(info)+ AvatarService,环扩 200——业务调用可长期存活。
pub const RM_TAP_V3_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var old=G['__caligo_tap_fns'];var removed=0;if(old){for(var i=0;i<old.length;i++){try{electron.ipcMain.removeListener(old[i].ch,old[i].fn);removed++}catch(err){}}}var chans=['RM_IPCFROM_RENDERER2','RM_IPCFROM_RENDERER4','RM_IPCFROM_RENDERER5','RM_IPCFROM_RENDERER6','RM_IPCFROM_RENDERER7'];G['__caligo_tap']=[];var fns=[];var rec=function(ch){return function(){try{var b=G['__caligo_tap'];if(!b){return}var a2=arguments[2];try{if(a2&&typeof a2==='object'){var cn=a2.cmdName;if(cn==='info'||(typeof cn==='string'&&cn.indexOf('nodeIKernelAvatarService')===0)){return}}}catch(err){}var e={ch:ch,t:Date.now(),argc:arguments.length,args:[]};for(var i=0;i<arguments.length;i++){var a=arguments[i];var t=typeof a;if(t==='object'&&a!==null){try{e.args.push({t:t,j:JSON.stringify(a).slice(0,8192)})}catch(err){e.args.push({t:t,j:'ERR:'+String(err).slice(0,80)})}}else{e.args.push({t:t,s:String(a).slice(0,512)})}}b.push(e);if(b.length>200){b.splice(0,b.length-200)}}catch(err){}}};for(var i=0;i<chans.length;i++){var f=rec(chans[i]);fns.push({ch:chans[i],fn:f});electron.ipcMain.on(chans[i],f)}G['__caligo_tap_fns']=fns;G['__caligo_r10']='waiting';return JSON.stringify({kicked:true,removed:removed,installed:fns.length})}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-10 追加:主消息窗口 renderer 深探——全量 window 键表(1243 键全名)+
/// dtResponseCallbacks 结构(对话后应已 populate)+ ipcImpl 类型/键。
/// 两段式 `__caligo_r11`,全部只读。
pub const RENDERER_DEEP_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r11'];if(st!==undefined&&st!=='waiting'){delete G['__caligo_r11'];return String(st).slice(0,64000)}if(st==='waiting'){return 'waiting'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){var o={ok:true};try{var wk=Object.getOwnPropertyNames(window);o.windowKeysAll=wk}catch(e){}try{var d=window.dtResponseCallbacks;if(d&&typeof d==='object'){var dk=Object.getOwnPropertyNames(d);o.dtcTotal=dk.length;o.dtcKeys=dk.slice(0,40);var first=dk[0];if(first){var fv=d[first];o.dtcValueType=typeof fv;if(fv&&typeof fv==='object'){o.dtcValueKeys=Object.getOwnPropertyNames(fv).slice(0,40)}}}}catch(e){o.dtcErr=String(e).slice(0,150)}try{o.ipcImplType=typeof window.ipcImpl;o.ipcImplKeys=window.ipcImpl&&typeof window.ipcImpl==='object'?Object.getOwnPropertyNames(window.ipcImpl).slice(0,80):undefined}catch(e){}try{o.electronType=typeof window.electron;o.electronKeys=window.electron&&typeof window.electron==='object'?Object.getOwnPropertyNames(window.electron).slice(0,80):undefined}catch(e){}return JSON.stringify(o)})()\";G['__caligo_r11']='waiting';target.executeJavaScript(probe,false).then(function(r){G['__caligo_r11']=String(r).slice(0,64000)},function(e){G['__caligo_r11']='ERR:'+String(e).slice(0,300)});return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-10 追加:ipcImpl/ipcRenderer 方法源文本(toString,只读)——暴露 IPC 路由。
pub const IPCIMPL_TEXT_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r12'];if(st!==undefined&&st!=='waiting'){delete G['__caligo_r12'];return String(st).slice(0,64000)}if(st==='waiting'){return 'waiting'}var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){function fninfo(name,f){var e={name:name};try{e.type=typeof f;if(typeof f==='function'){e.arity=f.length;var t=Function.prototype.toString.call(f);e.len=t.length;e.placeholder=t.indexOf('[native code]')>=0;e.snip=t.slice(0,600)}}catch(err){e.err=String(err).slice(0,100)}return e}var o={ok:true};try{var ii=window.ipcImpl;if(ii){o.ipcImpl={ctor:ii.constructor&&ii.constructor.name};o.send=fninfo('send',ii.send);o.on=fninfo('on',ii.on);o.removeAll=fninfo('removeAllListeners',ii.removeAllListeners)}}catch(e){o.iiErr=String(e).slice(0,120)}try{var ir=window.ipcRenderer;if(ir){o.ipcRenderer={ctor:ir.constructor&&ir.constructor.name,keys:Object.getOwnPropertyNames(ir).slice(0,60)};o.irSend=fninfo('send',ir.send);o.irInvoke=fninfo('invoke',ir.invoke);o.irOn=fninfo('on',ir.on);o.irPostMessage=fninfo('postMessage',ir.postMessage)}}catch(e){o.irErr=String(e).slice(0,120)}return JSON.stringify(o)})()\";G['__caligo_r12']='waiting';target.executeJavaScript(probe,false).then(function(r){G['__caligo_r12']=String(r).slice(0,64000)},function(e){G['__caligo_r12']='ERR:'+String(e).slice(0,300)});return 'kicked'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K2-10 追加:进程拓扑——_events 复查(对话后新通道?)+ utilityProcess 列表。
pub const PROCESS_TOPO_SCRIPT: &str = "(function(){try{var o={ok:true};var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return JSON.stringify({ok:false,error:'no require'})}var electron=req('electron');try{var ev=electron.ipcMain._events||{};var k=Object.getOwnPropertyNames(ev);o.eventChannels=k;o.channelCount=k.length}catch(e){}try{var up=electron.utilityProcess;if(up&&typeof up.getAllProcesses==='function'){var ps=up.getAllProcesses();o.utility=[];for(var i=0;i<ps.length;i++){o.utility.push({pid:ps[i].pid,type:ps[i].type})}}}catch(e){o.upErr=String(e).slice(0,150)}try{o.childProcessType=require('child_process')?'module':'?'}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// K2-10 收尾探针:process.moduleLoadList(纯读)——找 initIpc/qq-proton 注册名。
pub const MODULE_LOADLIST_SCRIPT: &str = "(function(){try{var o={ok:true};try{var ml=process.moduleLoadList;o.loadList=ml;o.loadCount=ml?ml.length:0}catch(e){o.mlErr=String(e).slice(0,150)}try{o.hasLinkedBindingFn=typeof process._linkedBinding}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({ok:false,error:String(e)})}})()";

/// 远程调用上下文(加载器写入,#[repr(C)]).
#[repr(C)]
pub struct AsyncCtx {
    /// 候选 node::Environment*(0 = 干跑:只验证解析与布局链,不 init async)。
    pub env: usize,
    /// 0=干跑 1=原子载荷 2=JS 枚举 3=中断捕获+执行(备用)。
    pub mode: u32,
    /// mode 2 的脚本选择:0=指纹 1=load 探针。
    pub script: u32,
    /// NUL 结尾 UTF-16 报告路径缓冲地址。
    pub report_path: usize,
    /// 等待回调触发的毫秒数。
    pub wait_ms: u32,
    pub _pad: u32,
}

// --- 回调侧状态(仅原子;缓冲在回调前就绪) ---
static FIRED: AtomicU32 = AtomicU32::new(0);
static CB_TID: AtomicU32 = AtomicU32::new(0);
static CB_TICK: AtomicU64 = AtomicU64::new(0);
static CB_ISOLATE_MATCH: AtomicU32 = AtomicU32::new(0);
static CB_HAS_CTX: AtomicU32 = AtomicU32::new(0); // 0=无 1=entered 2=incumbent
static CB_JS_ERR: AtomicU32 = AtomicU32::new(0); // 0=ok 1=newstring/compile空 2=run空 3=链断裂 4=导出缺失
static RESULT_LEN: AtomicU32 = AtomicU32::new(0);
static RESULT_SEQ: AtomicU32 = AtomicU32::new(0); // 0=未触发 1=已触发无结果 2=有结果
static CB_MODE: AtomicU32 = AtomicU32::new(0);
static CB_SCRIPT: AtomicU32 = AtomicU32::new(0);
static CB_ENV: AtomicUsize = AtomicUsize::new(0);
static CB_ISOLATE: AtomicUsize = AtomicUsize::new(0);
/// mode 3:中断点捕获的 tagged Context 指针(0 = 未捕获)。
static CB_CTX_TAG: AtomicU64 = AtomicU64::new(0);
/// mode 3 phase A 完成标记(1=捕获完毕 2=回调已跑但未捕获到上下文)。
static CAP_SEQ: AtomicU32 = AtomicU32::new(0);

/// mode 2 结果缓冲(回调写、远程线程读;回调发布 seq=2 后读方可见)。
/// 64 KiB:introspect 类探针(launcher 二级展开 + 符号键)输出可达数十 KB。
const RESULT_CAP: usize = 64 * 1024;
static mut RESULT_BUF: [u8; RESULT_CAP] = [0; RESULT_CAP];

// --- QQNT 导出(mangled 名;经 obs::export_addr 裸读解析) ---

const NAME_UV_ASYNC_INIT: &str = "uv_async_init";
const NAME_UV_ASYNC_SEND: &str = "uv_async_send";
const NAME_UV_HANDLE_SIZE: &str = "uv_handle_size";
const NAME_UV_LOOP_ALIVE: &str = "uv_loop_alive";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ISOLATE_ENTERED_CTX: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_ISOLATE_INCUMBENT_CTX: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_HANDLE_SCOPE_CREATE: &str =
    "?CreateHandle@HandleScope@v8@@KAPEA_KPEAVIsolate@2@_K@Z";
const NAME_REQUEST_INTERRUPT: &str =
    "?RequestInterrupt@node@@YAXPEAVEnvironment@1@P6AXPEAX@Z1@Z";
const NAME_HANDLE_SCOPE_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HANDLE_SCOPE_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_STRING_NEW_FROM_UTF8: &str =
    "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_SCRIPT_COMPILE: &str = "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
// Run 的单参导出是裸跳板(直跳双参本体且不准备 R9 → 残留 R9 会被当作
// Local<Data>);直接调用双参重载,data 传空 Local(0)。
const NAME_SCRIPT_RUN: &str =
    "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_CTOR: &str =
    "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";

type FnUvAsyncInit = unsafe extern "C" fn(
    loop_: *mut c_void,
    async_: *mut c_void,
    cb: unsafe extern "C" fn(*mut c_void),
) -> i32;
type FnUvAsyncSend = unsafe extern "C" fn(async_: *mut c_void) -> i32;
type FnUvHandleSize = unsafe extern "C" fn(t: i32) -> usize;
type FnUvLoopAlive = unsafe extern "C" fn(loop_: *mut c_void) -> i32;
type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut c_void;
/// v8::Local/MaybeLocal 非平凡返回的 MSVC/clang-cl ABI(全部门实测反汇编证据):
/// - **静态函数**:sret 在 RCX,参数自 RDX 起后移(NewFromUtf8 全参验证);
/// - **成员函数**:this 在 RCX,sret 在 RDX,其余参数自 R8 起
///   (GetEnteredOrMicrotaskContext 读 [rcx+0x100D0] + 写 [rdx];
///    Run 尾部 mov [rsi],rax,rsi=arg2)。第一版实弹两起事故均源于此序。
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnHandleScopeCtor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnHandleScopeCreate =
    unsafe extern "C" fn(isolate: *mut c_void, tagged: u64) -> *mut u64;
type FnRequestInterrupt =
    unsafe extern "C" fn(env: *mut c_void, cb: unsafe extern "system" fn(*mut c_void), ctx: *mut c_void);
type FnStringNewFromUtf8 = unsafe extern "C" fn(
    sret: *mut usize,
    isolate: *mut c_void,
    data: *const u8,
    ty: i32,
    len: i32,
);
type FnScriptCompile =
    unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun =
    unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
type FnUtf8Deref = unsafe extern "C" fn(this: *mut c_void) -> *const u8;

/// Local/MaybeLocal 空判定:8 字节包装,空即值为 0。
fn local_empty(v: usize) -> bool {
    v == 0
}

/// 调用成员函数风格的 Local 返回导出(this 在 RCX、sret 在 RDX),取回句柄值;
/// 0 = 空。
pub(crate) unsafe fn call_sret1(
    f: unsafe extern "C" fn(*mut c_void, *mut usize),
    this: *mut c_void,
) -> usize {
    let mut ret = 0usize;
    // SAFETY: sret 槽在调用方栈上;f 来自裸读导出表。
    unsafe { f(this, core::ptr::addr_of_mut!(ret)) };
    ret
}

#[allow(dead_code)]
struct Exports {
    uv_async_init: FnUvAsyncInit,
    uv_async_send: FnUvAsyncSend,
    uv_handle_size: FnUvHandleSize,
    uv_loop_alive: FnUvLoopAlive,
    isolate_get_current: FnIsolateGetCurrent,
    isolate_entered_ctx: FnIsolateCtx,
    isolate_incumbent_ctx: FnIsolateCtx,
    handle_scope_create: FnHandleScopeCreate,
    handle_scope_ctor: FnHandleScopeCtor,
    handle_scope_dtor: FnScopeDtor,
    string_new_from_utf8: FnStringNewFromUtf8,
    script_compile: FnScriptCompile,
    script_run: FnScriptRun,
    utf8_ctor: FnUtf8Ctor,
    utf8_dtor: FnScopeDtor,
    utf8_deref: FnUtf8Deref,
    request_interrupt: Option<FnRequestInterrupt>,
}

/// 地址 → 函数指针(签名由调用方按导出面 mangled 名保证)。
fn to_fn<T: Copy>(a: usize) -> T {
    // SAFETY: 地址来自裸读导出表;调用方以类型参数固定签名。
    // 经 usize 位面拷贝(泛型 T 上 transmute 无法静态证尺寸相等)。
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
    unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
}

unsafe fn resolve_exports(qqnt: usize, report: &str) -> Option<Exports> {
    macro_rules! get {
        ($name:expr) => {
            match crate::obs::export_addr(qqnt, $name) {
                Some(a) => a,
                None => {
                    append_stage(report, "resolve", false, &format!("export missing: {}", $name));
                    return None;
                }
            }
        };
    }
    Some(Exports {
        uv_async_init: to_fn(get!(NAME_UV_ASYNC_INIT)),
        uv_async_send: to_fn(get!(NAME_UV_ASYNC_SEND)),
        uv_handle_size: to_fn(get!(NAME_UV_HANDLE_SIZE)),
        uv_loop_alive: to_fn(get!(NAME_UV_LOOP_ALIVE)),
        isolate_get_current: to_fn(get!(NAME_ISOLATE_GETCURRENT)),
        isolate_entered_ctx: to_fn(get!(NAME_ISOLATE_ENTERED_CTX)),
        isolate_incumbent_ctx: to_fn(get!(NAME_ISOLATE_INCUMBENT_CTX)),
        handle_scope_create: to_fn(get!(NAME_HANDLE_SCOPE_CREATE)),
        handle_scope_ctor: to_fn(get!(NAME_HANDLE_SCOPE_CTOR)),
        handle_scope_dtor: to_fn(get!(NAME_HANDLE_SCOPE_DTOR)),
        string_new_from_utf8: to_fn(get!(NAME_STRING_NEW_FROM_UTF8)),
        script_compile: to_fn(get!(NAME_SCRIPT_COMPILE)),
        script_run: to_fn(get!(NAME_SCRIPT_RUN)),
        utf8_ctor: to_fn(get!(NAME_UTF8_CTOR)),
        utf8_dtor: to_fn(get!(NAME_UTF8_DTOR)),
        utf8_deref: to_fn(get!(NAME_UTF8_DEREF)),
        // mode 3 需要;缺失不阻塞 mode 0-2。
        request_interrupt: match crate::obs::export_addr(qqnt, NAME_REQUEST_INTERRUPT) {
            Some(a) => Some(to_fn(a)),
            None => {
                append_stage(report, "resolve", true, "RequestInterrupt missing (mode 3 unavailable)");
                None
            }
        },
    })
}

// --- 页校验读(与 intr 相同语义) ---

fn page_readable(addr: usize) -> bool {
    // SAFETY: VirtualQuery 无锁查询。
    unsafe {
        let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
            std::mem::zeroed();
        let n = windows_sys::Win32::System::Memory::VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut mbi,
            std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 {
            return false;
        }
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        mbi.State == MEM_COMMIT && (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) == 0
    }
}

fn page_readable_span(addr: usize, len: usize) -> bool {
    let start = addr & !0xFFF;
    let end = (addr + len - 1) & !0xFFF;
    let mut page = start;
    while page <= end {
        if !page_readable(page) {
            return false;
        }
        page += 0x1000;
    }
    true
}

unsafe fn read_usize(p: usize) -> Option<usize> {
    if !page_readable_span(p, 8) {
        return None;
    }
    // SAFETY: 页已校验;窗口期卸载风险由报告如实呈现。
    unsafe { Some((p as *const usize).read_unaligned()) }
}

/// 回调可见的导出束(远程线程在 uv_async_init 之前发布)。
static EXPORTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe fn strlen_bounded(p: *const u8, cap: usize) -> usize {
    let mut n = 0usize;
    // SAFETY: Utf8Value 缓冲 NUL 结尾;上限防失控。
    unsafe {
        while n < cap && *p.add(n) != 0 {
            n += 1;
        }
    }
    n
}

/// uv_async 回调:在 QQ 的 JS 线程、libuv 轮转点执行。
/// 全路径零 IO/零堆分配/零锁(导出束与缓冲均预先就绪)。
unsafe extern "C" fn async_cb(_handle: *mut c_void) {
    // 原子记录(全部模式)。
    // SAFETY: GetCurrentThreadId/GetTickCount64 无副作用。
    unsafe {
        CB_TID.store(
            windows_sys::Win32::System::Threading::GetCurrentThreadId(),
            Ordering::Release,
        );
        CB_TICK.store(
            windows_sys::Win32::System::SystemInformation::GetTickCount64(),
            Ordering::Release,
        );
    }
    FIRED.store(1, Ordering::Release);
    let mode = CB_MODE.load(Ordering::Acquire);
    if mode < 2 {
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    }

    // mode 2/3:JS 枚举。任何前置不满足 → 原子标记后返回,不执行 JS。
    let ex_ptr = EXPORTS.load(Ordering::Acquire);
    if ex_ptr == 0 {
        CB_JS_ERR.store(4, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    }
    let env = CB_ENV.load(Ordering::Acquire);
    let Some(env_isolate) = (unsafe { read_usize(env + 0xA0) }) else {
        CB_JS_ERR.store(3, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
        return;
    };
    // SAFETY: 导出束由远程线程在 init 前发布。
    let ex: &Exports = unsafe { &*(ex_ptr as *const Exports) };

    // HandleScope:v8 HandleScope 对象实际约 0x20 字节(isolate + prev Data),
    // 取 0x40 槽位防布局漂移踩栈。
    let mut scope = [0usize; 8];
    // SAFETY: ctor/dtor 成对;isolate 来自 env 布局链(页校验)。
    unsafe {
        (ex.handle_scope_ctor)(scope.as_mut_ptr().cast(), env_isolate as *mut c_void);
    }

    let finish = |js_err: u32| {
        CB_JS_ERR.store(js_err, Ordering::Release);
        RESULT_SEQ.store(if js_err == 0 { 2 } else { 1 }, Ordering::Release);
    };
    let bail = |js_err: u32| {
        CB_JS_ERR.store(js_err, Ordering::Release);
        RESULT_SEQ.store(1, Ordering::Release);
    };

    let ctx: usize;
    let isolate = env_isolate;
    if mode == 2 {
        // 上下文阶梯:GetCurrent 一致性 → entered → incumbent;皆空则安全返回。
        // SAFETY: 无参静态导出。
        let current = unsafe { (ex.isolate_get_current)() } as usize;
        let isolate_ok = current == 0 || current == env_isolate;
        CB_ISOLATE_MATCH.store(u32::from(isolate_ok), Ordering::Release);
        if !isolate_ok {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(3);
            return;
        }
        // SAFETY: 纯读访问器(sret 约定)。
        let entered = unsafe { call_sret1(ex.isolate_entered_ctx, isolate as *mut c_void) };
        let mut found = 0usize;
        let mut src_kind = 0u32;
        if !local_empty(entered) {
            found = entered;
            src_kind = 1;
        } else {
            let incumbent = unsafe { call_sret1(ex.isolate_incumbent_ctx, isolate as *mut c_void) };
            if !local_empty(incumbent) {
                found = incumbent;
                src_kind = 2;
            }
        }
        CB_HAS_CTX.store(src_kind, Ordering::Release);
        if found == 0 {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(0);
            return;
        }
        ctx = found;
    } else {
        // mode 3:中断点捕获的 tagged 指针 → CreateHandle 转为合法 Local。
        let tag = CB_CTX_TAG.load(Ordering::Acquire);
        if tag == 0 {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(5); // 5 = 未捕获到上下文
            return;
        }
        // SAFETY: 当前 HandleScope 内分配槽位;tagged 值来自中断点实读。
        let slot = unsafe { (ex.handle_scope_create)(isolate as *mut c_void, tag) };
        if slot.is_null() {
            // SAFETY: 成对析构。
            unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
            bail(3);
            return;
        }
        ctx = slot as usize;
    }
    // SAFETY: 助手只做 Compile/Run/Utf8,均在当前有效 HandleScope 内。
    let script_sel = CB_SCRIPT.load(Ordering::Acquire);
    let r = unsafe { exec_enum_script(ex, isolate, ctx, script_sel) };
    // SAFETY: 成对析构。
    unsafe { (ex.handle_scope_dtor)(scope.as_mut_ptr().cast()) };
    finish(r);
}

/// mode 3 phase A 回调:RequestInterrupt 的处理点(v8 执行 JS 的间隙)。
/// 只读 entered/incumbent Local 槽内的 tagged Context 指针并原子发布;
/// 不调用任何有副作用的 V8 API(计划 §3.1)。
unsafe extern "system" fn intr_capture_cb(_ctx: *mut c_void) {
    let ex_ptr = EXPORTS.load(Ordering::Acquire);
    if ex_ptr == 0 {
        CAP_SEQ.store(2, Ordering::Release);
        return;
    }
    // SAFETY: 导出束由远程线程在触发中断前发布。
    let ex: &Exports = unsafe { &*(ex_ptr as *const Exports) };
    let isolate = CB_ISOLATE.load(Ordering::Acquire);
    if isolate == 0 {
        CAP_SEQ.store(2, Ordering::Release);
        return;
    }
    // SAFETY: 纯读访问器(sret 约定)。
    let entered = unsafe { call_sret1(ex.isolate_entered_ctx, isolate as *mut c_void) };
    let mut tag = 0u64;
    if !local_empty(entered) {
        // Local 值 = 句柄槽地址;*槽 = tagged 对象指针(对象生命周期覆盖 env)。
        tag = unsafe { read_usize(entered) }.unwrap_or(0) as u64;
    } else {
        let incumbent = unsafe { call_sret1(ex.isolate_incumbent_ctx, isolate as *mut c_void) };
        if !local_empty(incumbent) {
            tag = unsafe { read_usize(incumbent) }.unwrap_or(0) as u64;
        }
    }
    if tag != 0 {
        CB_CTX_TAG.store(tag, Ordering::Release);
    }
    CAP_SEQ.store(1, Ordering::Release);
}

/// 在给定 isolate/context 上执行选定脚本并落结果缓冲。
/// 返回 js_err 码(0=成功)。须在有效 HandleScope 内调用。
unsafe fn exec_enum_script(ex: &Exports, isolate: usize, ctx: usize, script: u32) -> u32 {
    let src_str = match script {
        SCRIPT_LOAD_PROBE => LOAD_PROBE_SCRIPT,
        SCRIPT_LOAD_HARVEST => LOAD_HARVEST_SCRIPT,
        SCRIPT_GLOBAL_INTROSPECT => GLOBAL_INTROSPECT_SCRIPT,
        SCRIPT_MAINMODULE_PROBE => MAINMODULE_PROBE_SCRIPT,
        SCRIPT_ELECTRON_ENUM => ELECTRON_ENUM_SCRIPT,
        SCRIPT_RENDERER_BRIDGE => RENDERER_BRIDGE_SCRIPT,
        SCRIPT_API_MAP => API_MAP_SCRIPT,
        SCRIPT_WEBPACK_MAP => WEBPACK_MAP_SCRIPT,
        SCRIPT_IPCMAIN_MAP => IPCMAIN_MAP_SCRIPT,
        SCRIPT_HANDLER_TEXT => HANDLER_TEXT_SCRIPT,
        SCRIPT_RM_TAP => RM_TAP_SCRIPT,
        SCRIPT_RM_TAP_REMOVE => RM_TAP_REMOVE_SCRIPT,
        SCRIPT_RM_TAP_FILTERED => RM_TAP_FILTERED_SCRIPT,
        SCRIPT_INVOKE_HANDLERS => INVOKE_HANDLERS_SCRIPT,
        SCRIPT_RM_TAP_V3 => RM_TAP_V3_SCRIPT,
        SCRIPT_RENDERER_DEEP => RENDERER_DEEP_SCRIPT,
        SCRIPT_IPCIMPL_TEXT => IPCIMPL_TEXT_SCRIPT,
        SCRIPT_PROCESS_TOPO => PROCESS_TOPO_SCRIPT,
        SCRIPT_MODULE_LOADLIST => MODULE_LOADLIST_SCRIPT,
        SCRIPT_K3_C0 => K3_C0_SCRIPT,
        SCRIPT_K3_C1 => K3_C1_SCRIPT,
        SCRIPT_K3_DIAG => K3_DIAG_SCRIPT,
        SCRIPT_K3_UID => K3_UID_SCRIPT,
        SCRIPT_K3_PROBE_ROUTE => K3_PROBE_ROUTE_SCRIPT,
        SCRIPT_K3_SEND => K3_SEND_SCRIPT,
        SCRIPT_K3_DOM => K3_DOM_SCRIPT,
        SCRIPT_K3_DOM_INJECT => K3_DOM_INJECT_SCRIPT,
        SCRIPT_K3_DOM_INJECT2 => K3_DOM_INJECT2_SCRIPT,
        SCRIPT_K3_DOM_INJECT3 => K3_DOM_INJECT3_SCRIPT,
        SCRIPT_K3_DOM_SEND => K3_DOM_SEND_SCRIPT,
        SCRIPT_K3_SENDCHK => K3_SENDCHK_SCRIPT,
        SCRIPT_K3_SENDCHK2 => K3_SENDCHK2_SCRIPT,
        SCRIPT_K3_ENTER => K3_ENTER_SCRIPT,
        SCRIPT_K3_ENTERCHK => K3_ENTERCHK_SCRIPT,
        SCRIPT_K3_SENDINPUT => K3_SENDINPUT_SCRIPT,
        SCRIPT_K3_SI2 => K3_SI2_SCRIPT,
        SCRIPT_K3_WINSTATE => K3_WINSTATE_SCRIPT,
        SCRIPT_K3_RESTORE_SEND => K3_RESTORE_SEND_SCRIPT,
        SCRIPT_K3_RS2 => K3_RS2_SCRIPT,
        SCRIPT_K3_HEADLESS_RESP => K3_HEADLESS_RESP_SCRIPT,
        SCRIPT_K3_IPCIMPL_ON => K3_IPCIMPL_ON_SCRIPT,
        SCRIPT_K3_WCSEND_ARM => K3_WCSEND_ARM_SCRIPT,
        SCRIPT_K3_WCSEND_READ => K3_WCSEND_READ_SCRIPT,
        SCRIPT_K3_WCSEND_RESTORE => K3_WCSEND_RESTORE_SCRIPT,
        SCRIPT_K3_QQNT_KEYS => K3_QQNT_KEYS_SCRIPT,
        SCRIPT_K3_PROTO => K3_PROTO_SCRIPT,
        SCRIPT_K3_SESSION_TEST => K3_SESSION_TEST_SCRIPT,
        SCRIPT_K3_INIT_DISC => K3_INIT_DISC_SCRIPT,
        SCRIPT_K3_LIVE_SESSION => K3_LIVE_SESSION_SCRIPT,
        SCRIPT_K3_ENGINE_LIVE => K3_ENGINE_LIVE_SCRIPT,
        SCRIPT_K3_DEVINFO => K3_DEVINFO_SCRIPT,
        SCRIPT_K3_UIDKEY => K3_UIDKEY_SCRIPT,
        SCRIPT_K3_INITLADDER => K3_INITLADDER_SCRIPT,
        SCRIPT_K3_STARTUP => K3_STARTUP_SCRIPT,
        SCRIPT_K3_SESSIONID => K3_SESSIONID_SCRIPT,
        SCRIPT_K3_IDSHAPE => K3_IDSHAPE_SCRIPT,
        SCRIPT_K3_LIVE2 => K3_LIVE2_SCRIPT,
        SCRIPT_K3_STATICS_SWEEP => K3_STATICS_SWEEP_SCRIPT,
        SCRIPT_K3_USERDATA => K3_USERDATA_SCRIPT,
        SCRIPT_K3_ENGINELADDER => K3_ENGINELADDER_SCRIPT,
        SCRIPT_K3_ENG2 => K3_ENG2_SCRIPT,
        SCRIPT_K3_CIDCMD => K3_CIDCMD_SCRIPT,
        SCRIPT_K3_FOCUS_ENTER => K3_FOCUS_ENTER_SCRIPT,
        SCRIPT_K3_SNOW_TAP => K3_SNOW_TAP_SCRIPT,
        SCRIPT_K3_GROUPLIST2 => K3_GROUPLIST2_SCRIPT,
        SCRIPT_K3_DIRECT_HANDLER => K3_DIRECT_HANDLER_SCRIPT,
        SCRIPT_K3_DH2 => K3_DH2_SCRIPT,
        SCRIPT_K3_SENDLADDER => K3_SENDLADDER_SCRIPT,
        SCRIPT_K3_SEND3 => K3_SEND3_SCRIPT,
        SCRIPT_K3_ARITY => K3_ARITY_SCRIPT,
        SCRIPT_K3_SEND5 => K3_SEND5_SCRIPT,
        SCRIPT_K3_PROTOARITY => K3_PROTOARITY_SCRIPT,
        SCRIPT_K3_NAPSEND => K3_NAPSEND_SCRIPT,
        SCRIPT_K3_ONLINEDEV => K3_ONLINEDEV_SCRIPT,
        SCRIPT_K3_SESSIONKEY2 => K3_SESSIONKEY2_SCRIPT,
        SCRIPT_K3_LIVESVC => K3_LIVESVC_SCRIPT,
        SCRIPT_K3_SESSIONSCAN => K3_SESSIONSCAN_SCRIPT,
        SCRIPT_K3_DIRECTSEND => K3_DIRECTSEND_SCRIPT,
        SCRIPT_K3_DSREAD => K3_DSREAD_SCRIPT,
        SCRIPT_K3_RECV_ARM => K3_RECV_ARM_SCRIPT,
        SCRIPT_K3_RECV_ARM2 => K3_RECV_ARM2_SCRIPT,
        SCRIPT_K3_C2CSEND => K3_C2CSEND_SCRIPT,
        _ => ENUM_SCRIPT,
    };
    // String::NewFromUtf8(kNormal=0;sret 约定)。
    let script_bytes = src_str.as_bytes();
    let mut src = 0usize;
    // SAFETY: 只读静态缓冲 + 有效 isolate;sret 槽在栈上。
    unsafe {
        (ex.string_new_from_utf8)(
            core::ptr::addr_of_mut!(src),
            isolate as *mut c_void,
            script_bytes.as_ptr(),
            0,
            script_bytes.len() as i32,
        )
    };
    if local_empty(src) {
        return 1;
    }
    // ScriptOrigin(0x28 零构造;script_id=-1;host_defined 空 → ctor 尾调用短路)。
    let mut origin: [u8; 0x28] = [0; 0x28];
    origin[0x14..0x18].copy_from_slice(&(-1i32).to_le_bytes());
    // SAFETY: 参数均为刚取得的有效句柄(sret 约定)。
    let mut script = 0usize;
    unsafe {
        (ex.script_compile)(core::ptr::addr_of_mut!(script), ctx, src, origin.as_mut_ptr().cast())
    };
    if local_empty(script) {
        return 1;
    }
    // SAFETY: 方法调用(this, sret, ctx, data=空 Local 约定);script 为 Compile 产物。
    let mut result = 0usize;
    unsafe { (ex.script_run)(script, core::ptr::addr_of_mut!(result), ctx, 0) };
    if local_empty(result) {
        return 2;
    }
    // Utf8Value 取回(结构 0x18 槽位足够:ptr + length)。
    let mut utf8 = [0usize; 3];
    // SAFETY: ctor/dtor 成对。
    unsafe {
        (ex.utf8_ctor)(utf8.as_mut_ptr().cast(), isolate as *mut c_void, result);
    }
    let text = unsafe { (ex.utf8_deref)(utf8.as_mut_ptr().cast()) };
    if !text.is_null() {
        let len = unsafe { strlen_bounded(text, RESULT_CAP - 1) };
        // SAFETY: 目标为静态缓冲;长度已限。
        unsafe {
            let dst = core::ptr::addr_of_mut!(RESULT_BUF).cast::<u8>();
            core::ptr::copy_nonoverlapping(text, dst, len);
            *dst.add(len) = 0;
            RESULT_LEN.store(len as u32, Ordering::Release);
        }
    }
    // SAFETY: 成对析构。
    unsafe {
        (ex.utf8_dtor)(utf8.as_mut_ptr().cast());
    }
    0
}

fn read_wide(path_ptr: usize) -> Option<String> {
    if path_ptr == 0 {
        return None;
    }
    let p = path_ptr as *const u16;
    // SAFETY: loader 约定 NUL 结尾;扫描上限 32 KiB。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 {
            if *p.add(len) == 0 {
                break;
            }
            len += 1;
        }
        if len >= 32 * 1024 {
            return None;
        }
        String::from_utf16(std::slice::from_raw_parts(p, len)).ok()
    }
}

/// 远程线程主体。同步执行:解析 → 校验 → (mode≥1)init+send → 轮询 → 报告。
///
/// # Safety
///
/// ctx 必须指向本进程内由加载器写入的有效 [`AsyncCtx`];env 须来自
/// K2-02 扫描结果且实例健康。
pub unsafe fn async_run(ctx: &AsyncCtx) -> u32 {
    // 复位回调状态。
    FIRED.store(0, Ordering::Release);
    CB_TID.store(0, Ordering::Release);
    CB_TICK.store(0, Ordering::Release);
    CB_ISOLATE_MATCH.store(0, Ordering::Release);
    CB_HAS_CTX.store(0, Ordering::Release);
    CB_JS_ERR.store(0, Ordering::Release);
    RESULT_LEN.store(0, Ordering::Release);
    RESULT_SEQ.store(0, Ordering::Release);
    CB_MODE.store(0, Ordering::Release);
    CB_ENV.store(0, Ordering::Release);
    CB_SCRIPT.store(0, Ordering::Release);
    CB_ISOLATE.store(0, Ordering::Release);
    CB_CTX_TAG.store(0, Ordering::Release);
    CAP_SEQ.store(0, Ordering::Release);
    EXPORTS.store(0, Ordering::Release);

    let path_ptr = ctx.report_path;
    if path_ptr == 0 {
        return async_code::ERR_NULL_PATH;
    }
    let report = match read_wide(path_ptr) {
        Some(s) => s,
        None => return async_code::ERR_BAD_PATH,
    };
    if ctx.mode > 3 {
        append_stage(&report, "mode", false, &format!("bad mode {}", ctx.mode));
        return async_code::ERR_BAD_MODE;
    }
    append_stage(
        &report,
        "start",
        true,
        &format!("mode={} env={:#x} wait_ms={}", ctx.mode, ctx.env, ctx.wait_ms),
    );

    // QQNT 模块句柄:GetModuleHandleW 的 F1-R4 竞态是与宿主并发加载模块;
    // 本实验与 intr 同型(实例稳定运行、单次调用、窗口如实记录)。
    let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
    // SAFETY: 只读查询;竞态论证见上注。
    let h = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) };
    if h.is_null() {
        append_stage(&report, "qqnt", false, "QQNT.dll not loaded");
        return async_code::ERR_ENV_INVALID;
    }
    let Some(ex_boxed) = (unsafe { resolve_exports(h as usize, &report) }) else {
        return async_code::ERR_ENV_INVALID;
    };
    let ex = Box::leak(Box::new(ex_boxed));
    EXPORTS.store(ex as *const Exports as usize, Ordering::Release);

    // env 布局链:env+0xB0 → IsolateData → +0x11E8 → loop。
    if ctx.env == 0 {
        append_stage(&report, "dry_run", true, "env=0; exports resolved; plumbing only");
        append_stage(&report, "done", true, "dry run complete");
        return async_code::OK;
    }
    if !page_readable_span(ctx.env, 0xB60) {
        append_stage(&report, "validate_env", false, "env pages unreadable (need 0xB60 span)");
        return async_code::ERR_ENV_INVALID;
    }
    // 13056 事故修复:env 新鲜度校验(vfptr 必须等于 qqnt_base+主 vtable RVA)。
    // exec::EXPECTED_VTABLE_RVA 同源;qqnt 基址经 GetModuleHandleW 同前例。
    {
        let wide: Vec<u16> = "QQNT.dll\0".encode_utf16().collect();
        // SAFETY: 只读查询;竞态论证同前(K2-03 先例)。
        let h = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) };
        let qbase = if h.is_null() { 0 } else { h as usize };
        let vfptr_ok = qbase != 0 && (unsafe { crate::exec::env_is_fresh_pub(qbase, ctx.env) });
        if !vfptr_ok {
            append_stage(
                &report,
                "validate_env",
                false,
                "env STALE (vfptr mismatch) — rescan required (envscan)",
            );
            return async_code::ERR_ENV_INVALID;
        }
    }
    let Some(isolate_data) = (unsafe { read_usize(ctx.env + 0xB0) }) else {
        append_stage(&report, "validate_env", false, "env+0xB0 unreadable");
        return async_code::ERR_ENV_INVALID;
    };
    let Some(loop_) = (unsafe { read_usize(isolate_data + 0x11E8) }) else {
        append_stage(&report, "validate_env", false, "isolate_data+0x11E8 unreadable");
        return async_code::ERR_ENV_INVALID;
    };
    append_stage(
        &report,
        "validate_env",
        true,
        &format!("isolate_data={isolate_data:#x} loop={loop_:#x}"),
    );

    let alive = (ex.uv_loop_alive)(loop_ as *mut c_void);
    append_stage(&report, "loop_alive", alive != 0, &format!("alive={alive}"));
    if alive == 0 && ctx.mode >= 1 {
        append_stage(&report, "abort", true, "loop not alive; refusing async_init");
        return async_code::ERR_UV;
    }
    if ctx.mode == 0 {
        append_stage(&report, "done", true, "dry run complete");
        return async_code::OK;
    }

    // mode 3 phase A:中断点捕获上下文(JS 间隙必然 entered)。
    if ctx.mode == 3 {
        let Some(ri) = ex.request_interrupt else {
            append_stage(&report, "mode3", false, "RequestInterrupt export missing");
            return async_code::ERR_BAD_MODE;
        };
        let Some(env_isolate) = (unsafe { read_usize(ctx.env + 0xA0) }) else {
            append_stage(&report, "mode3", false, "env+0xA0 unreadable");
            return async_code::ERR_ENV_INVALID;
        };
        CAP_SEQ.store(0, Ordering::Release);
        CB_CTX_TAG.store(0, Ordering::Release);
        CB_ISOLATE.store(env_isolate, Ordering::Release);
        append_stage(&report, "mode3_capture", true, &format!("isolate={env_isolate:#x}"));
        // SAFETY: env 已页校验;回调只做纯读 + 原子写。
        unsafe { ri(ctx.env as *mut c_void, intr_capture_cb, std::ptr::null_mut()) };
        let cap_deadline = std::time::Instant::now() + std::time::Duration::from_millis((ctx.wait_ms.max(2) as u64) / 2);
        while CAP_SEQ.load(Ordering::Acquire) == 0 && std::time::Instant::now() < cap_deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let cap = CB_CTX_TAG.load(Ordering::Acquire);
        append_stage(
            &report,
            "mode3_capture",
            cap != 0,
            &format!("cap_seq={} ctx_tag={cap:#x}", CAP_SEQ.load(Ordering::Acquire)),
        );
        if cap == 0 {
            return async_code::ERR_NO_CAPTURE;
        }
    }

    // UV_ASYNC = 1(libuv uv_handle_type)。
    let size = (ex.uv_handle_size)(1);
    if size == 0 || size > 0x1000 {
        append_stage(&report, "alloc_async", false, &format!("uv_handle_size(UV_ASYNC)={size}"));
        return async_code::ERR_UV;
    }
    let layout = match std::alloc::Layout::from_size_align(size, 16) {
        Ok(l) => l,
        Err(_) => return async_code::ERR_UV,
    };
    let async_handle = unsafe { std::alloc::alloc(layout) };
    if async_handle.is_null() {
        append_stage(&report, "alloc_async", false, "alloc failed");
        return async_code::ERR_UV;
    }
    // SAFETY: 清零分配缓冲,防脏字段。
    unsafe {
        core::ptr::write_bytes(async_handle, 0, size);
    }

    CB_MODE.store(ctx.mode, Ordering::Release);
    CB_ENV.store(ctx.env, Ordering::Release);
    CB_SCRIPT.store(ctx.script, Ordering::Release);

    let init_r = unsafe {
        (ex.uv_async_init)(loop_ as *mut c_void, async_handle as *mut c_void, async_cb)
    };
    append_stage(&report, "uv_async_init", init_r == 0, &format!("r={init_r} loop={loop_:#x}"));
    if init_r != 0 {
        return async_code::ERR_UV;
    }
    let send_r = unsafe { (ex.uv_async_send)(async_handle as *mut c_void) };
    let started = std::time::Instant::now();
    append_stage(&report, "uv_async_send", send_r == 0, &format!("r={send_r}"));

    // 轮询(10ms 步进)。
    let deadline = started + std::time::Duration::from_millis(ctx.wait_ms.max(1) as u64);
    while RESULT_SEQ.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let fired = FIRED.load(Ordering::Acquire);
    let tid = CB_TID.load(Ordering::Acquire);
    let latency = started.elapsed().as_millis() as u64;
    append_stage(
        &report,
        "callback_fired",
        fired != 0,
        &format!("fired={fired} cb_thread_id={tid} latency_ms={latency}"),
    );
    if ctx.mode >= 2 {
        append_stage(
            &report,
            "context_ladder",
            true,
            &format!(
                "isolate_match={} has_ctx={} js_err={}",
                CB_ISOLATE_MATCH.load(Ordering::Acquire),
                CB_HAS_CTX.load(Ordering::Acquire),
                CB_JS_ERR.load(Ordering::Acquire),
            ),
        );
        if RESULT_SEQ.load(Ordering::Acquire) == 2 {
            let len = RESULT_LEN.load(Ordering::Acquire) as usize;
            // SAFETY: 回调已以 seq=2 发布;长度受缓冲上限约束。
            let text = unsafe {
                let src = core::ptr::addr_of!(RESULT_BUF).cast::<u8>();
                core::slice::from_raw_parts(src, len)
            };
            append_stage(&report, "result", true, &String::from_utf8_lossy(text));
        } else {
            append_stage(&report, "result", false, "no JS output (see context_ladder)");
        }
    }
    // 句柄不 close(需与 loop 同步;随进程退出回收——如实记录)。
    std::thread::sleep(std::time::Duration::from_millis(200));
    let still_ok = page_readable_span(ctx.env, 0x40);
    append_stage(&report, "post_check", still_ok, &format!("env still readable={still_ok}"));
    append_stage(&report, "done", true, "async round complete");
    async_code::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_script_is_self_contained() {
        // 只读枚举脚本:不赋值全局、不加定时器、有 try/catch、返回字符串。
        assert!(ENUM_SCRIPT.starts_with("(function(){"));
        assert!(ENUM_SCRIPT.contains("try"));
        assert!(ENUM_SCRIPT.contains("_linkedBinding('major')"));
        assert!(ENUM_SCRIPT.ends_with("}})()"));
        // load 探针:无参调用一次,异常捕获,不链式调用。
        assert!(LOAD_PROBE_SCRIPT.contains("m.load()"));
        assert!(LOAD_PROBE_SCRIPT.contains("callErr"));
        assert!(LOAD_PROBE_SCRIPT.contains("fnArity"));
        // 错误收割:只喂错误类型参数,不喂有效路径。
        assert!(LOAD_HARVEST_SCRIPT.contains("load(1)"));
        assert!(LOAD_HARVEST_SCRIPT.contains("globalCount"));
        assert!(!LOAD_HARVEST_SCRIPT.contains("app_launcher"));
    }

    #[test]
    fn ctx_layout_is_fixed() {
        // repr(C) 布局核对:env/mode/script/report/wait —— 与加载器写入序列一致。
        assert_eq!(std::mem::size_of::<AsyncCtx>(), std::mem::size_of::<usize>() * 3 + 8);
        let c = AsyncCtx {
            env: 0x11,
            mode: 2,
            script: 1,
            report_path: 0x22,
            wait_ms: 33,
            _pad: 0,
        };
        let base = &c as *const AsyncCtx as *const u8;
        // SAFETY: 读自身结构体字段。
        unsafe {
            assert_eq!(*(base as *const usize), 0x11);
            assert_eq!(*base.add(std::mem::size_of::<usize>()).cast::<u32>(), 2);
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() + 4).cast::<u32>(),
                1
            );
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() * 2).cast::<usize>(),
                0x22
            );
            assert_eq!(
                *base.add(std::mem::size_of::<usize>() * 3).cast::<u32>(),
                33
            );
        }
    }
}

/// K3-DOM 侦察:主窗口输入区/发送按钮候选选择器与 Vue 容器(纯只读)。
pub const K3_DOM_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r28'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r28']='READ_DONE:'+String(G['__caligo_r28_tmp']||'').slice(0,6000);return G['__caligo_r28']}if(!st){var probe=\"(function(){var o={};try{var cands=['textarea','[contenteditable=true]','.editor-wrapper','.ck-editor','.ql-editor','div[contenteditable]','.chat-input','.input-editor','.edit-area'];o.found=[];for(var i=0;i<cands.length;i++){var els=document.querySelectorAll(cands[i]);if(els.length){var e={sel:cands[i],n:els.length};var f=els[0];e.tag=f.tagName;e.cls=String(f.className).slice(0,100);e.aria=f.getAttribute('aria-label');e.ph=f.getAttribute('placeholder');e.textLen=(f.textContent||'').length;o.found.push(e)}}}catch(err){o.qErr=String(err).slice(0,150)}try{o.hasVue=document.querySelector('#app')&&!!document.querySelector('#app').__vue_app__}catch(e){}try{o.title=document.title}catch(e){}try{o.bodyLen=document.body?document.body.innerHTML.length:0}catch(e){}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r28_tmp']='OK:'+String(r).slice(0,5600)},function(e){G['__caligo_r28_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r28']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-DOM 注入:ProseMirror 编辑器 execCommand('insertText') 写入唯一正文,
/// 探测发送按钮(aria-label/类名),fallback 派发 Enter keydown。
/// 结果(写入后编辑器文本/按钮发现情况)回传主 env。
pub const K3_DOM_INJECT_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r29'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r29']='READ_DONE:'+String(G['__caligo_r29_tmp']||'').slice(0,6000);return G['__caligo_r29']}if(!st){var probe=\"(function(){var o={};try{var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){o.err='editor not found';return JSON.stringify(o)}ed.focus();document.execCommand('insertText',false,'CALIGO-K3-GROUP-001');o.textAfter=(ed.textContent||'').slice(0,100);var btns=document.querySelectorAll('[aria-label*=发送],[aria-label*=Send],[class*=send-btn],[class*=sendBtn],button');o.btnCands=[];for(var i=0;i<btns.length&&i<30;i++){var b=btns[i];var al=(b.getAttribute('aria-label')||'')+'|'+String(b.className).slice(0,60);if((b.getAttribute('aria-label')||'').indexOf('发送')>=0||(String(b.className)||'').indexOf('send')>=0){o.btnCands.push({i:i,desc:al.slice(0,120),tag:b.tagName})}}o.enterPlan=true}catch(err){o.err=String(err).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r29_tmp']='OK:'+String(r).slice(0,5600)},function(e){G['__caligo_r29_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r29']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-DOM 注入 v2:ProseMirror paste 事件(DataTransfer)写入正文;
/// textAfter 校验;按钮态复查。回车派发独立档。
pub const K3_DOM_INJECT2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r30'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r30']='READ_DONE:'+String(G['__caligo_r30_tmp']||'').slice(0,6000);return G['__caligo_r30']}if(!st){var probe=\"(function(){var o={};try{var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){o.err='editor not found';return JSON.stringify(o)}ed.focus();var text='CALIGO-K3-GROUP-001';var ok=false;try{var dt=new DataTransfer();dt.setData('text/plain',text);var pe=new ClipboardEvent('paste',{clipboardData:dt,bubbles:true,cancelable:true});ed.dispatchEvent(pe);ok=true;o.method='paste'}catch(e1){o.pasteErr=String(e1).slice(0,150)}o.textAfter=(ed.textContent||'').slice(0,100);var sb=document.querySelector('[class*=send-btn]');o.btnClass=sb?String(sb.className).slice(0,80):null}catch(err){o.err=String(err).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r30_tmp']='OK:'+String(r).slice(0,5600)},function(e){G['__caligo_r30_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r30']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-DOM 注入 v3:显式光标就位 + execCommand;fallback 直接 DOM 文本节点变异
/// (PM MutationObserver 同步);全程校验 textAfter 与按钮态。
pub const K3_DOM_INJECT3_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r31'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r31']='READ_DONE:'+String(G['__caligo_r31_tmp']||'').slice(0,6000);return G['__caligo_r31']}if(!st){var probe=\"(function(){var o={};try{var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){o.err='editor not found';return JSON.stringify(o)}var text='CALIGO-K3-GROUP-001';ed.focus();try{var sel=window.getSelection();var rg=document.createRange();rg.selectNodeContents(ed);rg.collapse(false);sel.removeAllRanges();sel.addRange(rg);o.caret='set'}catch(e0){o.caretErr=String(e0).slice(0,120)}try{document.execCommand('insertText',false,text);o.method='execcommand'}catch(e1){o.ecErr=String(e1).slice(0,120)}o.textAfter=(ed.textContent||'').slice(0,100);if(!o.textAfter){try{var tn=document.createTextNode(text);ed.appendChild(tn);o.method='dom-append'}catch(e2){o.daErr=String(e2).slice(0,120)}o.textAfter=(ed.textContent||'').slice(0,100)}var sb=document.querySelector('[class*=send-btn]');o.btnClass=sb?String(sb.className).slice(0,80):null}catch(err){o.err=String(err).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r31_tmp']='OK:'+String(r).slice(0,5600)},function(e){G['__caligo_r31_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r31']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-DOM 发送:点击真实发送按钮;校验编辑器清空(发送成功的 UI 信号)。
pub const K3_DOM_SEND_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r32'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r32']='READ_DONE:'+String(G['__caligo_r32_tmp']||'').slice(0,4000);return G['__caligo_r32']}if(!st){var probe=\"(function(){var o={};try{var ed=document.querySelector('[contenteditable=true].ProseMirror');o.textBefore=(ed&&ed.textContent||'').slice(0,60);var sb=document.querySelector('[class*=send-btn]');if(!sb){o.err='no send button';return JSON.stringify(o)}o.btnBefore=String(sb.className).slice(0,60);sb.click();o.clicked=true;o.btnAfter=String(sb.className).slice(0,60);setTimeout(function(){try{var ed2=document.querySelector('[contenteditable=true].ProseMirror');var sb2=document.querySelector('[class*=send-btn]');window.__caligo_sendchk={textAfter:(ed2&&ed2.textContent||'').slice(0,60),btnAfter:sb2?String(sb2.className).slice(0,60):null}}catch(e){}},800)}catch(err){o.err=String(err).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r32_tmp']='OK:'+String(r).slice(0,3600)},function(e){G['__caligo_r32_tmp']='REJECT:'+String(e).slice(0,300)});G['__caligo_r32']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 发送校验读取:取 __caligo_sendchk(延迟 800ms 的编辑器/按钮状态)。
pub const K3_SENDCHK_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}var probe=\"(function(){var W=window;return JSON.stringify({chk:W.__caligo_sendchk||null,textNow:(document.querySelector('[contenteditable=true].ProseMirror')||{textContent:''}).textContent.slice(0,60)})})()\";var out=null;target.executeJavaScript(probe,false).then(function(r){out=String(r).slice(0,600)},function(e){out='REJECT:'+String(e).slice(0,200)});var t0=Date.now();while(out===null&&Date.now()-t0<2500){}return out||'TIMEOUT'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 发送校验 v2(两段式):kick 读 __caligo_sendchk,then 写回主 env。
pub const K3_SENDCHK2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r34'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r34']='READ_DONE:'+String(G['__caligo_r34_tmp']||'').slice(0,1000);return G['__caligo_r34']}if(!st){var probe=\"(function(){var W=window;return JSON.stringify({chk:W.__caligo_sendchk||null,textNow:(document.querySelector('[contenteditable=true].ProseMirror')||{textContent:''}).textContent.slice(0,60)})})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r34_tmp']='OK:'+String(r).slice(0,800)},function(e){G['__caligo_r34_tmp']='REJECT:'+String(e).slice(0,200)});G['__caligo_r34']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 发送 v2:编辑器上派发 Enter keydown(keyCode 13,完整序列);
/// 延迟自查编辑器清空;两段式。
pub const K3_ENTER_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r35'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r35']='READ_DONE:'+String(G['__caligo_r35_tmp']||'').slice(0,1200);return G['__caligo_r35']}if(!st){var probe=\"(function(){var W=window;var o={};try{var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){o.err='editor not found';return JSON.stringify(o)}ed.focus();o.textBefore=(ed.textContent||'').slice(0,60);var ev=new KeyboardEvent('keydown',{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true,cancelable:true});ed.dispatchEvent(ev);o.enterSent=true;setTimeout(function(){try{var ed2=document.querySelector('[contenteditable=true].ProseMirror');var sb2=document.querySelector('[class*=send-btn]');W.__caligo_enterchk={textAfter:(ed2&&ed2.textContent||'').slice(0,60),btnAfter:sb2?String(sb2.className).slice(0,60):null}}catch(e){}},1000)}catch(err){o.err=String(err).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r35_tmp']='OK:'+String(r).slice(0,600)},function(e){G['__caligo_r35_tmp']='REJECT:'+String(e).slice(0,200)});G['__caligo_r35']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 Enter 后自查读取:取 __caligo_enterchk + 当前编辑器态。
pub const K3_ENTERCHK_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r36'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r36']='READ_DONE:'+String(G['__caligo_r36_tmp']||'').slice(0,1200);return G['__caligo_r36']}if(!st){var probe=\"(function(){var W=window;return JSON.stringify({chk:W.__caligo_enterchk||null,textNow:(document.querySelector('[contenteditable=true].ProseMirror')||{textContent:''}).textContent.slice(0,60)})})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r36_tmp']='OK:'+String(r).slice(0,800)},function(e){G['__caligo_r36_tmp']='REJECT:'+String(e).slice(0,200)});G['__caligo_r36']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 受信输入注入:sendInputEvent 点击编辑器(取 rect)→ 注入 Enter。
/// Chromium 层受信事件;两段式;自查编辑器清空。
pub const K3_SENDINPUT_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r37'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r37']='READ_DONE:'+String(G['__caligo_r37_tmp']||'').slice(0,1500);return G['__caligo_r37']}if(!st){var probe=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){return 'NOEDITOR'}var r=ed.getBoundingClientRect();window.__caligo_clickdone=false;return JSON.stringify({x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),text:(ed.textContent||'').slice(0,60)})})()\";target.executeJavaScript(probe,false).then(function(rv){try{var pos=JSON.parse(String(rv).replace(/^OK:/,''));if(pos&&typeof pos.x==='number'){target.sendInputEvent({type:'mouseDown',x:pos.x,y:pos.y,button:'left',clickCount:1});target.sendInputEvent({type:'mouseUp',x:pos.x,y:pos.y,button:'left',clickCount:1});setTimeout(function(){try{target.sendInputEvent({type:'keyDown',keyCode:'Enter'});target.sendInputEvent({type:'keyUp',keyCode:'Enter'});setTimeout(function(){try{var probe2=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');var sb=document.querySelector('[class*=send-btn]');return JSON.stringify({textAfter:(ed&&ed.textContent||'').slice(0,60),btn:sb?String(sb.className).slice(0,60):null})})()\";target.executeJavaScript(probe2,false).then(function(r2){G['__caligo_r37_tmp']='OK:'+String(r2).slice(0,900)},function(e){G['__caligo_r37_tmp']='REJECT2:'+String(e).slice(0,200)})}catch(e){G['__caligo_r37_tmp']='ERR3:'+String(e).slice(0,200)}},1200)}catch(e){G['__caligo_r37_tmp']='ERR2:'+String(e).slice(0,200)}},400)}else{G['__caligo_r37_tmp']='OK:nopos:'+String(rv).slice(0,200)}}catch(e){G['__caligo_r37_tmp']='ERRP:'+String(e).slice(0,200)+' raw:'+String(rv).slice(0,150)}},function(e){G['__caligo_r37_tmp']='REJECT:'+String(e).slice(0,200)});G['__caligo_r37']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 仪表化:窗口态 → rect → 点击 → activeElement 验证 → Enter → 文本校验。
pub const K3_SI2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r38'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r38']='READ_DONE:'+String(G['__caligo_r38_tmp']||'').slice(0,2000);return G['__caligo_r38']}if(!st){var win=target.getOwnerBrowserWindow&&target.getOwnerBrowserWindow();G['__caligo_r38_tmp']='pre:win='+(win?(win.isMinimized()?'min':(win.isVisible()?'vis':'hidden')):'null');var probe=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){return 'NOEDITOR'}var r=ed.getBoundingClientRect();return JSON.stringify({x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width),h:Math.round(r.height),text:(ed.textContent||'').slice(0,40),url:location.hash})})()\";target.executeJavaScript(probe,false).then(function(rv){try{var pos=JSON.parse(String(rv));if(pos&&typeof pos.x==='number'&&pos.w>0){target.sendInputEvent({type:'mouseDown',x:pos.x,y:pos.y,button:'left',clickCount:1});target.sendInputEvent({type:'mouseUp',x:pos.x,y:pos.y,button:'left',clickCount:1});setTimeout(function(){try{var probe2=\"(function(){var ae=document.activeElement;var ed=document.querySelector('[contenteditable=true].ProseMirror');return JSON.stringify({activeIsEditor:ae===ed,activeTag:ae?ae.tagName:null,focused:ed===document.activeElement})})()\";target.executeJavaScript(probe2,false).then(function(fv){G['__caligo_r38_tmp']+= ';focus='+String(fv).slice(0,120);try{target.sendInputEvent({type:'keyDown',keyCode:'Enter'});target.sendInputEvent({type:'keyUp',keyCode:'Enter'});setTimeout(function(){try{var probe3=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');var sb=document.querySelector('[class*=send-btn]');return JSON.stringify({textAfter:(ed&&ed.textContent||'').slice(0,60),btn:sb?String(sb.className).slice(0,60):null})})()\";target.executeJavaScript(probe3,false).then(function(r3){G['__caligo_r38_tmp']+=';after='+String(r3).slice(0,200)},function(e){G['__caligo_r38_tmp']+=';r3rej='+String(e).slice(0,120)})}catch(e){G['__caligo_r38_tmp']+=';e3='+String(e).slice(0,120)}},1200)}catch(e){G['__caligo_r38_tmp']+=';e2='+String(e).slice(0,120)}}),function(e){G['__caligo_r38_tmp']+=';f2rej='+String(e).slice(0,120)})}catch(e){G['__caligo_r38_tmp']+=';e1='+String(e).slice(0,120)}},400)}else{G['__caligo_r38_tmp']+=';nopos='+String(rv).slice(0,150)}}catch(e){G['__caligo_r38_tmp']+=';perr='+String(e).slice(0,150)}},function(e){G['__caligo_r38_tmp']+=';rej='+String(e).slice(0,150)});G['__caligo_r38']='waiting';return 'kicked:'+String(G['__caligo_r38_tmp']||'')}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 小探针:窗口态 + 编辑器 rect + 当前文本。
pub const K3_WINSTATE_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r39'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r39']='READ_DONE:'+String(G['__caligo_r39_tmp']||'').slice(0,1000);return G['__caligo_r39']}if(!st){var w=target.getOwnerBrowserWindow?target.getOwnerBrowserWindow():null;var ws=w?(w.isMinimized()?'MINIMIZED':(w.isVisible()?'VISIBLE':'HIDDEN')):'NOWIN';var probe=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed)return 'NOEDITOR';var r=ed.getBoundingClientRect();return JSON.stringify({x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width),text:(ed.textContent||'').slice(0,40)})})()\";target.executeJavaScript(probe,false).then(function(rv){G['__caligo_r39_tmp']='WIN='+ws+' RECT='+String(rv).slice(0,400)},function(e){G['__caligo_r39_tmp']='WIN='+ws+' REJ='+String(e).slice(0,150)});G['__caligo_r39']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 restore+send:恢复窗口 → sendInputEvent 点击编辑器 → Enter → 自查。
pub const K3_RESTORE_SEND_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r40'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r40']='READ_DONE:'+String(G['__caligo_r40_tmp']||'').slice(0,1500);return G['__caligo_r40']}if(!st){var w=target.getOwnerBrowserWindow?target.getOwnerBrowserWindow():null;if(w){if(w.isMinimized()){w.restore()}if(!w.isVisible()){w.show()}G['__caligo_r40_tmp']='restored'}else{G['__caligo_r40_tmp']='nowin'}var probe=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed)return 'NOEDITOR';var r=ed.getBoundingClientRect();return JSON.stringify({x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width)})})()\";target.executeJavaScript(probe,false).then(function(rv){try{var pos=JSON.parse(String(rv));if(pos&&pos.w>0){target.sendInputEvent({type:'mouseDown',x:pos.x,y:pos.y,button:'left',clickCount:1});target.sendInputEvent({type:'mouseUp',x:pos.x,y:pos.y,button:'left',clickCount:1});setTimeout(function(){try{target.sendInputEvent({type:'keyDown',keyCode:'Enter'});target.sendInputEvent({type:'keyUp',keyCode:'Enter'});setTimeout(function(){try{var p3=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');var sb=document.querySelector('[class*=send-btn]');return JSON.stringify({textAfter:(ed&&ed.textContent||'').slice(0,60),btn:sb?String(sb.className).slice(0,60):null})})()\";target.executeJavaScript(p3,false).then(function(r3){G['__caligo_r40_tmp']+=';after='+String(r3).slice(0,300)},function(e){G['__caligo_r40_tmp']+=';r3rej'})}catch(e){G['__caligo_r40_tmp']+=';e3'}},1500)}catch(e){G['__caligo_r40_tmp']+=';e2'}},500)}else{G['__caligo_r40_tmp']+=';rect0='+String(rv).slice(0,120)}}catch(e){G['__caligo_r40_tmp']+=';pe'}},function(e){G['__caligo_r40_tmp']+=';rej'});G['__caligo_r40']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 restore+send v2:restore → 延迟 1s → rect → 点击 → Enter → 自查。
pub const K3_RS2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r41'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r41']='READ_DONE:'+String(G['__caligo_r41_tmp']||'').slice(0,1500);return G['__caligo_r41']}if(!st){var w=target.getOwnerBrowserWindow?target.getOwnerBrowserWindow():null;if(w){if(w.isMinimized()){w.restore()}if(!w.isVisible()){w.show()}w.focus&&w.focus()}G['__caligo_r41_tmp']='restored';setTimeout(function(){try{var p1=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed)return 'NOEDITOR';var r=ed.getBoundingClientRect();return JSON.stringify({x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width)})})()\";target.executeJavaScript(p1,false).then(function(rv){try{var pos=JSON.parse(String(rv));if(pos&&pos.w>0){target.sendInputEvent({type:'mouseDown',x:pos.x,y:pos.y,button:'left',clickCount:1});target.sendInputEvent({type:'mouseUp',x:pos.x,y:pos.y,button:'left',clickCount:1});setTimeout(function(){try{target.sendInputEvent({type:'keyDown',keyCode:'Enter'});target.sendInputEvent({type:'keyUp',keyCode:'Enter'});setTimeout(function(){try{var p3=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');var sb=document.querySelector('[class*=send-btn]');return JSON.stringify({textAfter:(ed&&ed.textContent||'').slice(0,60),btn:sb?String(sb.className).slice(0,60):null})})()\";target.executeJavaScript(p3,false).then(function(r3){G['__caligo_r41_tmp']+=';after='+String(r3).slice(0,300)},function(){G['__caligo_r41_tmp']+=';r3rej'})}catch(e){G['__caligo_r41_tmp']+=';e3'}},1500)}catch(e){G['__caligo_r41_tmp']+=';e2'}},500)}else{G['__caligo_r41_tmp']+=';rect0='+String(rv).slice(0,120)}}catch(e){G['__caligo_r41_tmp']+=';pe'}},function(){G['__caligo_r41_tmp']+=';rej'})}catch(e){G['__caligo_r41_tmp']+=';e1'}},1000);G['__caligo_r41']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 headless 响应捕获:dtc 注册回调 + ipcImpl.send('ntApi',cmd,cid)(3 参)。
/// 若响应按 cid 投递则 getUidByUin 响应可见。纯 renderer,零窗口操作。
pub const K3_HEADLESS_RESP_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r42'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r42']='READ_DONE:'+String(G['__caligo_r42_tmp']||'').slice(0,4000);return G['__caligo_r42']}if(!st){var probe=\"(function(){var W=window;var o={};try{W.__caligo_hresp=[];var cid='hd-'+((W.crypto&&W.crypto.randomUUID)?W.crypto.randomUUID():Date.now());W.dtResponseCallbacks[cid]=function(){try{var a=Array.prototype.slice.call(arguments);var e={cid:cid,args:[]};for(var i=0;i<a.length;i++){var v=a[i];var t=typeof v;e.args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,2048):String(v).slice(0,512)})}W.__caligo_hresp.push(e)}catch(err){}};o.cid=cid;o.dtcN=Object.getOwnPropertyNames(W.dtResponseCallbacks).length;o.r1=W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]},cid);o.r1t=typeof o.r1}catch(e){o.err=String(e).slice(0,300)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r42_tmp']='FIRE:'+String(r).slice(0,1200)},function(e){G['__caligo_r42_tmp']='REJECT:'+String(e).slice(0,300)});setTimeout(function(){try{var p2=\"(function(){var W=window;return JSON.stringify({resp:W.__caligo_hresp||[],n:(W.__caligo_hresp||[]).length,dtcN:W.dtResponseCallbacks?Object.getOwnPropertyNames(W.dtResponseCallbacks).length:-1})})()\";target.executeJavaScript(p2,false).then(function(r2){G['__caligo_r42_tmp']+=';poll='+String(r2).slice(0,3000)},function(e){G['__caligo_r42_tmp']+=';prej='+String(e).slice(0,150)})}catch(e){G['__caligo_r42_tmp']+=';e2'}},2500);G['__caligo_r42']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 headless v2:ipcImpl.on('ntApi',cb) 验证(响应若走同名事件即现形)
/// + 多候选事件名监听;cb 触发次数/参数回传。纯 renderer。
pub const K3_IPCIMPL_ON_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r43'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r43']='READ_DONE:'+String(G['__caligo_r43_tmp']||'').slice(0,5000);return G['__caligo_r43']}if(!st){var probe=\"(function(){var W=window;var o={};try{W.__caligo_onresp=[];var evs=['ntApi','ntApiResponse','ntApiReply','onNtApi'];o.armed={};for(var i=0;i<evs.length;i++){try{var ev=evs[i];W.ipcImpl.on(ev,function(e2){return function(){try{var a=Array.prototype.slice.call(arguments);var e={ev:e2,n:a.length,args:[]};for(var i=0;i<a.length;i++){var v=a[i];var t=typeof v;e.args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,2048):String(v).slice(0,400)})}W.__caligo_onresp.push(e)}catch(err){}}}(ev));o.armed[ev]='ok'}catch(e){o.armed[ev]='ERR:'+String(e).slice(0,80)}}o.onKeys=Object.getOwnPropertyNames(W.ipcImpl);var cid='on-'+Date.now();W.dtResponseCallbacks[cid]=function(){try{W.__caligo_onresp.push({ev:'dtc',args:[JSON.stringify(Array.prototype.slice.call(arguments)).slice(0,2048)]})}catch(err){}};try{W.ipcImpl.send('ntApi',{cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',payload:[3089665724]});o.sent=true}catch(e){o.sendErr=String(e).slice(0,150)}}catch(e){o.err=String(e).slice(0,200)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r43_tmp']='FIRE:'+String(r).slice(0,1500)},function(e){G['__caligo_r43_tmp']='REJECT:'+String(e).slice(0,300)});setTimeout(function(){try{var p2=\"(function(){var W=window;return JSON.stringify({resp:W.__caligo_onresp||[],n:(W.__caligo_onresp||[]).length})})()\";target.executeJavaScript(p2,false).then(function(r2){G['__caligo_r43_tmp']+=';poll='+String(r2).slice(0,3000)},function(e){G['__caligo_r43_tmp']+=';prej'})}catch(e){G['__caligo_r43_tmp']+=';e2'}},2500);G['__caligo_r43']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 wc.send 补丁(加性,可还原):记录主→renderer 全部通道推送。
/// script 42=安装,43=读取(主 env 环形 80 条),44=还原。
pub const K3_WCSEND_ARM_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(G.__caligo_wclog){return 'ALREADY'}var orig=target.send;G.__caligo_wcorig=orig;G.__caligo_wclog=[];target.send=function(ch){try{var args=[];for(var i=1;i<arguments.length;i++){var v=arguments[i];var t=typeof v;args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,1200):String(v).slice(0,200)})}G.__caligo_wclog.push({ch:String(ch),n:args.length,args:args});if(G.__caligo_wclog.length>80){G.__caligo_wclog.splice(0,G.__caligo_wclog.length-80)}}catch(e){}return orig.apply(target,arguments)};return 'PATCHED'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 wc.send 读取:返回主 env 环形缓冲(安装后通道推送记录)。
pub const K3_WCSEND_READ_SCRIPT: &str = "(function(){try{var G=globalThis;var log=G.__caligo_wclog;if(!log){return 'NOT_ARMED'}var out=JSON.stringify({n:log.length,items:log});G.__caligo_wclog=[];return out.slice(0,64000)}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 wc.send 还原:恢复原函数,删除标记与缓冲。
pub const K3_WCSEND_RESTORE_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(!G.__caligo_wcorig){return 'NOT_ARMED'}target.send=G.__caligo_wcorig;delete G.__caligo_wcorig;delete G.__caligo_wclog;return 'RESTORED'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 终局侦察(纯只读):QQNT/major linkedBinding 导出键表 + process/globalThis
/// 中疑似总线对象的函数名扫描。
pub const K3_QQNT_KEYS_SCRIPT: &str = "(function(){try{var o={};try{var q=process._linkedBinding('QQNT');o.qqntType=typeof q;o.qqntKeys=q&&typeof q==='object'?Object.getOwnPropertyNames(q).slice(0,150):undefined;if(q&&typeof q==='object'){o.qqntTypes={};var ks=Object.getOwnPropertyNames(q);for(var i=0;i<ks.length&&i<80;i++){o.qqntTypes[ks[i]]=typeof q[ks[i]]}}}catch(e){o.qqntErr=String(e).slice(0,200)}try{var m=process._linkedBinding('major');o.majorKeys=Object.getOwnPropertyNames(m)}catch(e){}try{o.procBusLike={};var pk=Object.getOwnPropertyNames(process);for(var j=0;j<pk.length;j++){var n=pk[j];if(/ipc|bus|nt|kernel|native|bridge|init/i.test(n)){o.procBusLike[n]=typeof process[n]}}}catch(e){}try{o.gBusLike={};var gk=Object.getOwnPropertyNames(globalThis);for(var k=0;k<gk.length;k++){var n2=gk[k];if(/ipc|bus|ntapi|kernel|nativebridge/i.test(n2)){o.gBusLike[n2]=typeof globalThis[n2]}}}catch(e){}return JSON.stringify(o)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 服务类原型枚举(纯只读):Session/Engine/MsgService/GroupService 方法面。
pub const K3_PROTO_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');function proto(name){try{var C=q[name];if(!C)return {missing:true};var e={ctorParams:C.length};var p=C.prototype;e.methods=Object.getOwnPropertyNames(p).filter(function(n){return n!=='constructor'});e.statics=Object.getOwnPropertyNames(C).filter(function(n){return n!=='prototype'&&n!=='length'&&n!=='name'});return e}catch(err){return {err:String(err).slice(0,120)}}}o.session=proto('NodeIQQNTWrapperSession');o.engine=proto('NodeIQQNTWrapperEngine');o.msg=proto('NodeIKernelMsgService');o.group=proto('NodeIKernelGroupService');o.recent=proto('NodeIKernelRecentContactService');return JSON.stringify(o).slice(0,30000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 会话实例化验证:新建 Session(不 init)→ getSessionId/getMsgService/
/// getGroupService 可用性。只读方法;不调 init(生命周期零副作用)。
pub const K3_SESSION_TEST_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');try{var s=new q.NodeIQQNTWrapperSession();o.sessionCreated=true;try{o.sessionId=s.getSessionId()}catch(e){o.sidErr=String(e).slice(0,200)}try{var ms=s.getMsgService();o.msgType=typeof ms;o.msgHasSend=ms&&typeof ms.sendMsg==='function';o.msgCtor=ms&&ms.constructor?ms.constructor.name:null}catch(e){o.msgErr=String(e).slice(0,200)}try{var gs=s.getGroupService();o.groupType=typeof gs;o.groupHasGetList=gs&&typeof gs.getGroupList==='function'}catch(e){o.groupErr=String(e).slice(0,200)}try{var rc=s.getRecentContactService();o.recentType=typeof rc;o.recentHasSync=rc&&typeof rc.getRecentContactListSync==='function'}catch(e){o.recentErr=String(e).slice(0,200)}}catch(e){o.ctorErr=String(e).slice(0,300)}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 init 签名发现:engine.get() 形态 + session.init() 空参错误阶梯。
pub const K3_INIT_DISC_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');try{var e=q.NodeIQQNTWrapperEngine.get();o.engineGetType=typeof e;o.engineGetKeys=e&&typeof e==='object'?Object.getOwnPropertyNames(e).slice(0,80):String(e).slice(0,100)}catch(err){o.engineGetErr=String(err).slice(0,250)}try{var s=new q.NodeIQQNTWrapperSession();try{s.init();o.initNoArgs='returned'}catch(e1){o.initErr1=String(e1).slice(0,400)}try{s.init({});o.initEmpty='returned'}catch(e2){o.initErr2=String(e2).slice(0,400)}}catch(err){o.sessErr=String(err).slice(0,250)}try{o.sessStatics=Object.getOwnPropertyNames(q.NodeIQQNTWrapperSession).filter(function(n){return n!=='prototype'&&n!=='length'&&n!=='name'})}catch(e){}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 活会话获取:静态 getNTWrapperSession() → getSessionId/getMsgService 验证。
pub const K3_LIVE_SESSION_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;try{o.stType=typeof S.getNTWrapperSession;o.stArity=S.getNTWrapperSession.length;var ls=S.getNTWrapperSession();o.liveType=typeof ls;o.liveCtor=ls&&ls.constructor?ls.constructor.name:null;o.liveKeys=ls&&typeof ls==='object'?Object.getOwnPropertyNames(ls).slice(0,60):String(ls).slice(0,100);try{o.sid=ls.getSessionId()}catch(e1){o.sidErr=String(e1).slice(0,200)}try{var ms=ls.getMsgService();o.msgType=typeof ms;o.msgHasSend=!!(ms&&ms.sendMsg);o.msgKeys=ms&&typeof ms==='object'?Object.getOwnPropertyNames(Object.getPrototypeOf(ms)).slice(0,40):undefined}catch(e2){o.msgErr=String(e2).slice(0,250)}try{var gs=ls.getGroupService();o.groupOk=!!(gs&&gs.getGroupList)}catch(e3){o.groupErr=String(e3).slice(0,200)}}catch(e0){o.callErr=String(e0).slice(0,300)}return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 引擎活体验证 + getNTWrapperSession 参数阶梯。
pub const K3_ENGINE_LIVE_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var E=q.NodeIQQNTWrapperEngine;var S=q.NodeIQQNTWrapperSession;try{var e=E.get();o.eType=typeof e;try{o.devInfo=String(e.getDeviceInfo()).slice(0,600)}catch(e1){o.devErr=String(e1).slice(0,250)}try{o.protoKeys=Object.getOwnPropertyNames(Object.getPrototypeOf(e)).slice(0,40)}catch(e2){}}catch(e0){o.engErr=String(e0).slice(0,250)}var t=function(n,f){try{var r=f();o[n]={t:typeof r,v:String(r).slice(0,200)}}catch(err){o[n]={err:String(err).slice(0,250)}}};t('gws0',function(){return S.getNTWrapperSession(0)});t('gwsEmpty',function(){return S.getNTWrapperSession('')});t('gwsUin',function(){return S.getNTWrapperSession('1694717255')});return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 devInfo 全文 + 会话键阶梯(deviceId/uin/常见键)。
pub const K3_DEVINFO_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var E=q.NodeIQQNTWrapperEngine;var S=q.NodeIQQNTWrapperSession;var e=E.get();try{o.dev=JSON.stringify(e.getDeviceInfo()).slice(0,2500)}catch(e1){o.devErr=String(e1).slice(0,250)}var t=function(n,f){try{var r=f();o[n]={t:typeof r,has:(r&&typeof r==='object')?'obj':String(r).slice(0,60)}}catch(err){o[n]={err:String(err).slice(0,200)}}};var dev=null;try{dev=e.getDeviceInfo();var dk=dev?Object.getOwnPropertyNames(dev):[];o.devKeys=dk;for(var i=0;i<dk.length&&i<15;i++){var v=dev[dk[i]];if(typeof v==='string'||typeof v==='number'){try{var r=S.getNTWrapperSession(v);o['gws_'+dk[i]]=typeof r}catch(e2){o['gwsE_'+dk[i]]=String(e2).slice(0,120)}}}}catch(e3){}t('gws1',function(){return S.getNTWrapperSession(1)});return JSON.stringify(o).slice(0,6000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 会话键=authData.uid:读 uid/uin 标识(票据字段不读)→ getNTWrapperSession(uid)。
pub const K3_UIDKEY_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var a=globalThis.authData;o.authKeys=a?Object.getOwnPropertyNames(a):null;var uid=a&&a.uid?String(a.uid):'';var uin=a&&a.uin?String(a.uin):'';o.uidLen=uid.length;o.uidPrefix=uid.slice(0,2);try{var ls=S.getNTWrapperSession(uid);o.byUid=typeof ls;if(ls&&typeof ls==='object'){try{o.sid=String(ls.getSessionId()).slice(0,60)}catch(e1){o.sidErr=String(e1).slice(0,150)}try{var ms=ls.getMsgService();o.msgOk=!!(ms&&ms.sendMsg)}catch(e2){o.msgErr=String(e2).slice(0,150)}}}catch(e0){o.byUidErr=String(e0).slice(0,200)}if(uin){try{var ls2=S.getNTWrapperSession(uin);o.byUin=typeof ls2}catch(e4){o.byUinErr=String(e4).slice(0,150)}}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 init 参数类型阶梯:逐位喂错误类型,校验错误暴露 4 参签名。
pub const K3_INITLADDER_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var s=new q.NodeIQQNTWrapperSession();var t=function(n,f){try{f();o[n]='RETURNED'}catch(e){o[n]=String(e).slice(0,300)}};t('i1234',function(){s.init(1,2,3,4)});t('iX234',function(){s.init('x',2,3,4)});t('iXX34',function(){s.init('x','y',3,4)});t('iXXX4',function(){s.init('x','y','z',4)});t('iXXXX',function(){s.init('x','y','z','w')});t('iO234',function(){s.init({},2,3,4)});t('iOO34',function(){s.init({},{},3,4)});t('iOOO4',function(){s.init({},{},{},4)});t('iOOOO',function(){s.init({},{},{},{})});try{o.engineGetArity=q.NodeIQQNTWrapperEngine.get.length}catch(e){}return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 StartupSessionWrapper:原型+静态枚举 + 无参调用尝试。
pub const K3_STARTUP_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var C=q.NodeIQQNTStartupSessionWrapper;o.exists=typeof C;if(C){try{o.proto=Object.getOwnPropertyNames(C.prototype).filter(function(n){return n!=='constructor'})}catch(e1){o.protoErr=String(e1).slice(0,150)}try{o.statics=Object.getOwnPropertyNames(C).filter(function(n){return n!=='prototype'&&n!=='length'&&n!=='name'})}catch(e2){}try{o.ctorArity=C.length}catch(e3){}try{var i=C.get();o.getType=typeof i;if(i&&typeof i==='object'){try{o.iProto=Object.getOwnPropertyNames(Object.getPrototypeOf(i)).slice(0,60)}catch(e4){}}}catch(e5){o.getErr=String(e5).slice(0,250)}}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 会话 ID 列表:create() → getSessionIdList() → getNTWrapperSession(id)。
pub const K3_SESSIONID_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var W=q.NodeIQQNTStartupSessionWrapper;var w=W.create();o.wType=typeof w;try{o.ids=w.getSessionIdList();o.idsT=typeof o.ids}catch(e1){o.listErr=String(e1).slice(0,250)}var S=q.NodeIQQNTWrapperSession;if(o.ids&&o.ids.length){var id=o.ids[0];o.firstId=String(id).slice(0,60);try{var ls=S.getNTWrapperSession(id);o.sessType=typeof ls;if(ls&&typeof ls==='object'){try{o.sid=String(ls.getSessionId()).slice(0,60)}catch(e2){o.sidErr=String(e2).slice(0,150)}try{var ms=ls.getMsgService();o.msgOk=!!(ms&&ms.sendMsg)}catch(e3){o.msgErr=String(e3).slice(0,150)}try{var gs=ls.getGroupService();o.groupOk=!!(gs&&gs.getGroupList)}catch(e4){o.groupErr=String(e4).slice(0,150)}}}catch(e5){o.gwsErr=String(e5).slice(0,250)}}return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 会话 ID 集合形态:id 对象的键/值/原型,提取全部会话 ID。
pub const K3_IDSHAPE_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var w=q.NodeIQQNTStartupSessionWrapper.create();var ids=w.getSessionIdList();o.t=typeof ids;o.ctor=ids&&ids.constructor?ids.constructor.name:null;o.isArr=Array.isArray(ids);try{o.len=ids.length}catch(e1){}try{o.size=ids.size}catch(e2){}try{o.keys=Object.getOwnPropertyNames(ids).slice(0,40)}catch(e3){}try{o.proto=Object.getOwnPropertyNames(Object.getPrototypeOf(ids)).slice(0,40)}catch(e4){}try{if(typeof ids.get==='function'){var pk=Object.getOwnPropertyNames(ids);o.viaGet={};for(var i=0;i<pk.length&&i<10;i++){try{o.viaGet[pk[i]]=String(ids.get(pk[i])).slice(0,60)}catch(e5){}}}}catch(e6){}try{if(typeof ids.forEach==='function'){var collected=[];ids.forEach(function(v,k){collected.push(String(k)+'='+String(v).slice(0,50));if(collected.length>10)return});o.forEach=collected}}catch(e7){}try{if(typeof ids.entries==='function'){o.entries=Array.from(ids.entries()).slice(0,10).map(function(p){return String(p[0])+'='+String(p[1]).slice(0,50)})}}catch(e8){}return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 活会话验证:getNTWrapperSession('nt_3') → 服务实例可用性(只读检查)。
pub const K3_LIVE2_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var ls=S.getNTWrapperSession('nt_3');o.type=typeof ls;if(ls&&typeof ls==='object'){try{o.sid=String(ls.getSessionId()).slice(0,60)}catch(e1){o.sidErr=String(e1).slice(0,150)}try{var ms=ls.getMsgService();o.msgType=typeof ms;o.msgHasSend=!!(ms&&ms.sendMsg);o.msgHasGetMsgs=!!(ms&&ms.getMsgs);o.msgHasListener=!!(ms&&ms.addKernelMsgListener)}catch(e2){o.msgErr=String(e2).slice(0,200)}try{var gs=ls.getGroupService();o.groupOk=!!(gs&&gs.getGroupList)}catch(e3){o.groupErr=String(e3).slice(0,200)}try{var rc=ls.getRecentContactService();o.recentOk=!!(rc&&rc.getRecentContactListSync)}catch(e4){o.recentErr=String(e4).slice(0,200)}}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 全类静态扫描:QQNT 绑定全部类的 statics,找 session/id 相关全局访问器。
pub const K3_STATICS_SWEEP_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var ks=Object.getOwnPropertyNames(q);o.classCount=ks.length;o.hits={};for(var i=0;i<ks.length;i++){var C=q[ks[i]];if(!C||typeof C!=='function'){continue}try{var st=Object.getOwnPropertyNames(C).filter(function(n){return n!=='prototype'&&n!=='length'&&n!=='name'});for(var j=0;j<st.length;j++){if(/session|getNT|engine|instance|current|global/i.test(st[j])){if(!o.hits[ks[i]]){o.hits[ks[i]]=[]}if(o.hits[ks[i]].length<8){o.hits[ks[i]].push(st[j]+(typeof C[st[j]]==='function'?'('+C[st[j]].length+')':':'+typeof C[st[j]]))}}}}catch(e){}}return JSON.stringify(o).slice(0,20000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 用户数据配置:getNTUserDataInfoConfig() 全文。
pub const K3_USERDATA_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');try{var c=q.NodeQQNTWrapperUtil.getNTUserDataInfoConfig();o.type=typeof c;o.json=JSON.stringify(c).slice(0,3000)}catch(e){o.err=String(e).slice(0,300)}return JSON.stringify(o).slice(0,3500)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 引擎配置阶梯:initWithDeskTopConfig 空参/带 dataPath 的校验错误。
pub const K3_ENGINELADDER_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var e=q.NodeIQQNTWrapperEngine.get();var t=function(n,f){try{var r=f();o[n]='RETURNED:'+String(r).slice(0,100)}catch(err){o[n]=String(err).slice(0,350)}};t('empty',function(){return e.initWithDeskTopConfig({})});t('dp',function(){return e.initWithDeskTopConfig({dataPath:'C:\\Users\\Vegetable\\Documents\\Tencent Files'})});t('dp2',function(){return e.initWithDeskTopConfig({platform:'windows',dataPath:'C:\\Users\\Vegetable\\Documents\\Tencent Files'})});return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 引擎配置 2 参阶梯:组合类型探底。
pub const K3_ENG2_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var e=q.NodeIQQNTWrapperEngine.get();var t=function(n,f){try{var r=f();o[n]='RETURNED:'+String(r).slice(0,100)}catch(err){o[n]=String(err).slice(0,350)}};t('oO',function(){return e.initWithDeskTopConfig({},function(){})});t('OO',function(){return e.initWithDeskTopConfig({},{})});t('ss',function(){return e.initWithDeskTopConfig('a','b')});t('nO',function(){return e.initWithDeskTopConfig(1,function(){})});return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 cid-in-cmd:dtc 注册 + cmd 内嵌 callbackId 的 getUidByUin(总线响应回投)。
pub const K3_CIDCMD_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r44'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r44']='READ_DONE:'+String(G['__caligo_r44_tmp']||'').slice(0,6000);return G['__caligo_r44']}if(!st){var probe=\"(function(){var W=window;var o={};try{W.__caligo_cresp=[];var cid='cc-'+((W.crypto&&W.crypto.randomUUID)?W.crypto.randomUUID():Date.now());W.dtResponseCallbacks[cid]=function(){try{var a=Array.prototype.slice.call(arguments);var e={via:'dtc',n:a.length,args:[]};for(var i=0;i<a.length;i++){var v=a[i];var t=typeof v;e.args.push({t:t,j:t==='object'?JSON.stringify(v).slice(0,3000):String(v).slice(0,600)})}W.__caligo_cresp.push(e)}catch(err){}};var cmd={cmdName:'nodeIKernelProfileService/getUidByUin',cmdType:'invoke',callbackId:cid,payload:[3089665724]};W.ipcImpl.send('ntApi',cmd);o.sentWithCid=true;o.cid=cid}catch(e){o.err=String(e).slice(0,250)}return JSON.stringify(o)})()\";target.executeJavaScript(probe,false).then(function(r){G['__caligo_r44_tmp']='FIRE:'+String(r).slice(0,800)},function(e){G['__caligo_r44_tmp']='REJECT:'+String(e).slice(0,250)});setTimeout(function(){try{var p2=\"(function(){var W=window;return JSON.stringify({resp:W.__caligo_cresp||[],n:(W.__caligo_cresp||[]).length})})()\";target.executeJavaScript(p2,false).then(function(r2){G['__caligo_r44_tmp']+=';poll='+String(r2).slice(0,5000)},function(e){G['__caligo_r44_tmp']+=';prej'})}catch(e){G['__caligo_r44_tmp']+=';e2'}},3000);G['__caligo_r44']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 headless 发送:JS 聚焦编辑器 → sendInputEvent 受信 Enter(无坐标)→ 自查。
pub const K3_FOCUS_ENTER_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r45'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r45']='READ_DONE:'+String(G['__caligo_r45_tmp']||'').slice(0,1500);return G['__caligo_r45']}if(!st){var p1=\"(function(){var W=window;var ed=document.querySelector('[contenteditable=true].ProseMirror');if(!ed){return 'NOEDITOR'}ed.focus();var sel=W.getSelection();var rg=document.createRange();rg.selectNodeContents(ed);rg.collapse(false);sel.removeAllRanges();sel.addRange(rg);return JSON.stringify({focused:document.activeElement===ed,text:(ed.textContent||'').slice(0,60)})})()\";target.executeJavaScript(p1,false).then(function(rv){try{G['__caligo_r45_tmp']='focus='+String(rv).slice(0,200);if(String(rv).indexOf('\"focused\":true')>=0||String(rv).indexOf(\"'focused':'true\")>=0||String(rv).indexOf('true')>=0){target.sendInputEvent({type:'keyDown',keyCode:'Enter'});target.sendInputEvent({type:'keyUp',keyCode:'Enter'});G['__caligo_r45_tmp']+=';enterSent';setTimeout(function(){try{var p3=\"(function(){var ed=document.querySelector('[contenteditable=true].ProseMirror');var sb=document.querySelector('[class*=send-btn]');return JSON.stringify({textAfter:(ed&&ed.textContent||'').slice(0,60),btn:sb?String(sb.className).slice(0,60):null})})()\";target.executeJavaScript(p3,false).then(function(r3){G['__caligo_r45_tmp']+=';after='+String(r3).slice(0,300)},function(){G['__caligo_r45_tmp']+=';r3rej'})}catch(e){G['__caligo_r45_tmp']+=';e3'}},1500)}else{G['__caligo_r45_tmp']+=';notfocused'}}catch(e){G['__caligo_r45_tmp']+=';pe='+String(e).slice(0,150)}},function(e){G['__caligo_r45_tmp']='REJECT:'+String(e).slice(0,200)});G['__caligo_r45']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-Snowluma 观测 tap:保留 LogApi(接收路径 trace),只滤 Avatar;环 400。
pub const K3_SNOW_TAP_SCRIPT: &str = "(function(){try{var G=globalThis;var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var old=G['__caligo_tap_fns'];var removed=0;if(old){for(var i=0;i<old.length;i++){try{electron.ipcMain.removeListener(old[i].ch,old[i].fn);removed++}catch(err){}}}var chans=['RM_IPCFROM_RENDERER2','RM_IPCFROM_RENDERER4','RM_IPCFROM_RENDERER5','RM_IPCFROM_RENDERER6','RM_IPCFROM_RENDERER7'];G['__caligo_tap']=[];var fns=[];var rec=function(ch){return function(){try{var b=G['__caligo_tap'];if(!b){return}var a2=arguments[2];try{if(a2&&typeof a2==='object'){var cn=a2.cmdName;if(typeof cn==='string'&&cn.indexOf('nodeIKernelAvatarService')===0){return}}}catch(err){}var e={ch:ch,t:Date.now(),argc:arguments.length,args:[]};for(var i=0;i<arguments.length;i++){var a=arguments[i];var t=typeof a;if(t==='object'&&a!==null){try{e.args.push({t:t,j:JSON.stringify(a).slice(0,8192)})}catch(err){e.args.push({t:t,j:'ERR:'+String(err).slice(0,80)})}}else{e.args.push({t:t,s:String(a).slice(0,600)})}}b.push(e);if(b.length>400){b.splice(0,b.length-400)}}catch(err){}}};for(var i=0;i<chans.length;i++){var f=rec(chans[i]);fns.push({ch:chans[i],fn:f});electron.ipcMain.on(chans[i],f)}G['__caligo_tap_fns']=fns;G['__caligo_tap_state']='snow';return JSON.stringify({kicked:true,removed:removed,installed:fns.length})}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 闭环验证:RM 发 getGroupList(带 cid)→ wc.send 钩子抓响应 → 提取群 peerUid。

/// K3 闭环验证 v2:cid 经 window 全局传递;RM 发 getGroupList;wc 钩子抓响应。
pub const K3_GROUPLIST2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r47'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r47']='READ_DONE:'+String(G['__caligo_r47_tmp']||'').slice(0,64000);return G['__caligo_r47']}if(!st){var cid='gl-'+((target.crypto&&target.crypto.randomUUID)?target.crypto.randomUUID():Date.now());G['__caligo_glcid']=cid;var p1=\"(function(c){window.__caligo_cid=c;return 'cid-set'})(\"+JSON.stringify(cid)+\")\";target.executeJavaScript(p1,false).then(function(){try{var p2=\"(function(){var W=window;var f={type:'frame',sender:{ipc:{},navigationHistory:{},_events:{},_eventsCount:1},senderFrame:{},frameId:1,processId:5,frameTreeNodeId:2};var q={type:'request',callbackId:W.__caligo_cid,eventName:'ntApi',peerId:2};var c={cmdName:'nodeIKernelGroupService/getGroupList',cmdType:'invoke',payload:[false,100]};W.ipcRenderer.send('RM_IPCFROM_RENDERER2',f,q,c);return 'sent-'+W.__caligo_cid})()\";return target.executeJavaScript(p2,false)}catch(e){return Promise.resolve('ERR2:'+String(e).slice(0,200))}}).then(function(r){G['__caligo_r47_tmp']='FIRE:'+String(r).slice(0,300)},function(e){G['__caligo_r47_tmp']='REJECT:'+String(e).slice(0,250)});G['__caligo_r47']='waiting';return 'kicked cid='+cid}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 直调 RM handler:主 env 内以 fabricated event(sender=真 wc2)调 handler,
/// 响应经已补丁的 wc.send 现形。
pub const K3_DIRECT_HANDLER_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r48'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r48']='READ_DONE:'+String(G['__caligo_r48_tmp']||'').slice(0,64000);return G['__caligo_r48']}if(!st){var h=electron.ipcMain._events&&electron.ipcMain._events['RM_IPCFROM_RENDERER2'];if(!h){return 'ERR:no handler'}if(Array.isArray(h)){h=h[h.length-1]}var cid='dh-'+Date.now();G['__caligo_dhcid']=cid;var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html',top:{url:'app://./renderer/index.html'}},frameId:1,processId:5,frameTreeNodeId:2};var request={type:'request',callbackId:cid,eventName:'ntApi',peerId:2};var cmd={cmdName:'nodeIKernelGroupService/getGroupList',cmdType:'invoke',payload:[false,100]};var r=null;try{r=h(fakeEvent,request,cmd);G['__caligo_r48_tmp']='handlerReturned:'+typeof r+' '+String(r).slice(0,100)}catch(e1){G['__caligo_r48_tmp']='handlerThrew:'+String(e1).slice(0,400)}G['__caligo_r48']='waiting';return 'kicked cid='+cid+' '+String(G['__caligo_r48_tmp']||'').slice(0,200)}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 直调 v2:监听器数组首个(QQ 原始 handler,排除自家 tap),逐一尝试。
pub const K3_DH2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r49'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r49']='READ_DONE:'+String(G['__caligo_r49_tmp']||'').slice(0,64000);return G['__caligo_r49']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];o={count:list.length,types:[]};for(var i=0;i<list.length;i++){o.types.push(typeof list[i])}var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];o.realIndex=j;break}}if(!real){o.err='only taps found';G['__caligo_r49']=JSON.stringify(o);return G['__caligo_r49']}o.realIndex=o.realIndex!==undefined?o.realIndex:'?';var cid='dh2-'+Date.now();G['__caligo_dhcid']=cid;var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};var request={type:'request',callbackId:cid,eventName:'ntApi',peerId:2};var cmd={cmdName:'nodeIKernelGroupService/getGroupList',cmdType:'invoke',payload:[false,100]};try{var r=real(fakeEvent,request,cmd);G['__caligo_r49_tmp']='idx'+o.realIndex+' returned:'+typeof r}catch(e1){G['__caligo_r49_tmp']='idx'+o.realIndex+' threw:'+String(e1).slice(0,400)}G['__caligo_r49']='waiting';return 'kicked cid='+cid}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 最终发送:直调 RM handler 的 sendMsg(peerUid=群号,响应可见)。
/// 正文 CALIGO-K3-GROUP-001;单发;响应 errMsg 指导参数。
pub const K3_SEND3_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r52'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r52']='READ_DONE:'+String(G['__caligo_r52_tmp']||'').slice(0,64000);return G['__caligo_r52']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];break}}if(!real){return 'ERR:no real handler'}var cid='sm3-'+Date.now();G['__caligo_smcid']=cid;var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};var request={type:'request',callbackId:cid,eventName:'ntApi',peerId:2};var cmd={cmdName:'nodeIKernelMsgService/sendMsg',cmdType:'invoke',payload:[0,{chatType:2,guildId:'',peerUid:'263402786',peerUin:'263402786',msgId:'0'},[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]]};try{var r=real(fakeEvent,request,cmd);G['__caligo_r52_tmp']='fired:'+typeof r}catch(e1){G['__caligo_r52_tmp']='threw:'+String(e1).slice(0,400)}G['__caligo_r52']='waiting';return 'kicked cid='+cid}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";


/// K3 sendMsg 参数阶梯:单一脚本按序尝试多种 payload 形态,间隔轮询 wc 环,
/// 全部结果落 __caligo_r51。成功(result:0/full)即停。
pub const K3_SENDLADDER_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r51'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r51']='READ_DONE:'+String(G['__caligo_r51_tmp']||'').slice(0,64000);return G['__caligo_r51']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];break}}if(!real){return 'ERR:no real handler'}var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};var results=[];var variants=[['v3',[0,{chatType:2,guildId:'',peerUid:'263402786',peerUin:'263402786',msgId:'0'},[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]]],['v1',[{chatType:2,guildId:'',peerUid:'263402786',peerUin:'263402786',msgType:0,subMsgType:0,elements:[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]}]],['v2',[{chatType:2,guildId:'',peerUid:'263402786',peerUin:'263402786',msgId:'0'},[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]]],['v4',[2,{chatType:2,guildId:'',peerUid:'263402786',peerUin:'263402786',msgId:'0'},[{type:1,textElement:{content:'CALIGO-K3-GROUP-001'}}]]]];var idx=0;function next(){if(idx>=variants.length){G['__caligo_r51_tmp']='DONE:'+JSON.stringify(results).slice(0,4000);return}var v=variants[idx];var cid='sl'+idx+'-'+Date.now();var before=G.__caligo_wclog?G.__caligo_wclog.length:0;try{real(fakeEvent,{type:'request',callbackId:cid,eventName:'ntApi',peerId:2},{cmdName:'nodeIKernelMsgService/sendMsg',cmdType:'invoke',payload:v[1]});results.push({v:v[0],cid:cid,fired:true})}catch(e){results.push({v:v[0],threw:String(e).slice(0,200)})}setTimeout(function(){var log=G.__caligo_wclog||[];var found=null;for(var i=log.length-1;i>=0;i--){try{var a1=log[i].args;if(a1&&a1[0]&&String(a1[0].j).indexOf(cid)>=0){found={ch:log[i].ch,a0:a1[0].j,a1:a1[1]?a1[1].j:null};break}}}catch(e){}results[results.length-1].resp=found;idx++;next()},2000)}next();G['__caligo_r51']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 arity 探针:MsgService/GroupService 关键方法的 .length + 关键字段类型标记。
pub const K3_ARITY_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var s=new q.NodeIQQNTWrapperSession();try{var ms=s.getMsgService();o.sendMsgArity=ms.sendMsg.length;o.getMsgsArity=ms.getMsgs?ms.getMsgs.length:null;o.recallArity=ms.recallMsg?ms.recallMsg.length:null;o.sendMsgProtoName=ms.sendMsg.name}catch(e1){o.msErr=String(e1).slice(0,200)}try{var gs=s.getGroupService();o.getGroupListArity=gs.getGroupList.length}catch(e2){}try{var rc=s.getRecentContactService();o.recentSyncArity=rc.getRecentContactListSync?rc.getRecentContactListSync.length:null}catch(e3){}return JSON.stringify(o).slice(0,2000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 sendMsg v5:完整 schema struct(字段名对齐接收消息)+ elements 内嵌,2 参。
pub const K3_SEND5_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r53'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r53']='READ_DONE:'+String(G['__caligo_r53_tmp']||'').slice(0,64000);return G['__caligo_r53']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];break}}if(!real){return 'ERR:no real handler'}var cid='sm5-'+Date.now();var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};var request={type:'request',callbackId:cid,eventName:'ntApi',peerId:2};var struct={chatType:2,guildId:'',channelId:'',peerUid:'263402786',peerUin:'263402786',msgType:0,subMsgType:0,sendType:0,msgId:'0',msgSeq:'',cntSeq:'0',msgRandom:'',msgTime:'',fromUid:'',fromAppid:'',msgMeta:{},sendStatus:0,elements:[{elementType:1,elementId:'',elementGroupId:0,extBufForUI:{},textElement:{content:'CALIGO-K3-GROUP-001',atType:0,atUid:'0',atTinyId:'0',atNtUid:'',subElementType:0,atChannelId:'0',linkInfo:null,atRoleId:'0',atRoleColor:0,atRoleName:'',needNotify:0},faceElement:null,marketFaceElement:null,replyElement:null,picElement:null,pttElement:null,videoElement:null,grayTipElement:null,arkElement:null,fileElement:null}]};var cmd={cmdName:'nodeIKernelMsgService/sendMsg',cmdType:'invoke',payload:[0,struct]};try{var r=real(fakeEvent,request,cmd);G['__caligo_r53_tmp']='fired:'+typeof r}catch(e1){G['__caligo_r53_tmp']='threw:'+String(e1).slice(0,400)}G['__caligo_r53']='waiting';return 'kicked cid='+cid}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 原型 arity:无需实例,直接读 prototype 方法的 .length。
pub const K3_PROTOARITY_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var mp=q.NodeIKernelMsgService.prototype;o.sendMsgArity=mp.sendMsg?mp.sendMsg.length:null;o.sendMsgName=mp.sendMsg?mp.sendMsg.name:null;o.getMsgsArity=mp.getMsgs?mp.getMsgs.length:null;o.generateMsgUniqueIdArity=mp.generateMsgUniqueId?mp.generateMsgUniqueId.length:null;var gp=q.NodeIKernelGroupService.prototype;o.getGroupListArity=gp.getGroupList?gp.getGroupList.length:null;o.getGroupDetailInfoArity=gp.getGroupDetailInfo?gp.getGroupDetailInfo.length:null;var rp=q.NodeIKernelRecentContactService.prototype;o.recentSyncArity=rp.getRecentContactListSync?rp.getRecentContactListSync.length:null;var ap=q.NodeIKernelAvatarService.prototype;o.avatarArity=ap.getMembersAvatarPath?ap.getMembersAvatarPath.length:null;return JSON.stringify(o).slice(0,2000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 NapCat 式发送链:getServerTime → generateMsgUniqueId → sendMsg('0',peer,elems,Map)。
/// 全部经 RM 直调,响应读 wc 环;全 headless。
pub const K3_NAPSEND_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r55'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r55']='READ_DONE:'+String(G['__caligo_r55_tmp']||'').slice(0,64000);return G['__caligo_r55']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];break}}if(!real){return 'ERR:no real handler'}var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};function fire(cmdName,payload){var cid='ns-'+cmdName+'-'+Date.now();real(fakeEvent,{type:'request',callbackId:cid,eventName:'ntApi',peerId:2},{cmdName:cmdName,cmdType:'invoke',payload:payload});return cid}function readResp(cid,waitMs,cb){var t0=Date.now();function poll(){var log=G.__caligo_wclog||[];for(var i=log.length-1;i>=0;i--){try{var a0=log[i].args[0];if(a0&&String(a0.j).indexOf(cid)>=0){var cmdObj=null;try{cmdObj=JSON.parse(log[i].args[1].j)}catch(e){}cb({ch:log[i].ch,cmd:cmdObj,raw:String(log[i].args[1]?log[i].args[1].j:'').slice(0,1500)});return}}catch(e){}}if(Date.now()-t0>waitMs){cb(null);return}setTimeout(poll,300)}poll()}var log=[];var c1=fire('nodeIKernelMSFService/getServerTime',[]);readResp(c1,3000,function(r1){log.push({step:'serverTime',r:r1});var sv=0;try{if(r1){sv=(r1.cmd&&typeof r1.cmd==='object')?(r1.cmd.result||0):(r1.raw||0)}}catch(e){}var c2=fire('nodeIKernelMsgService/generateMsgUniqueId',[2,sv]);readResp(c2,3000,function(r2){log.push({step:'genId',r:r2});var mid=0;try{if(r2){mid=(r2.cmd&&typeof r2.cmd==='object')?(r2.cmd.result||r2.cmd.data||0):(r2.raw||0)}}catch(e){}var peer={chatType:2,guildId:String(mid),peerUid:'263402786',peerUin:'263402786'};var elems=[{elementType:1,textElement:{content:'CALIGO-K3-GROUP-001'}}];var c3=fire('nodeIKernelMsgService/sendMsg',['0',peer,elems,new Map()]);readResp(c3,5000,function(r3){log.push({step:'sendMsg',r:r3,mid:String(mid).slice(0,40)});G['__caligo_r55_tmp']='LOG:'+JSON.stringify(log).slice(0,8000)})})});G['__caligo_r55']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 分发器封锁验证:getOnlineDev(只读 MsgService 方法)经 RM 可达性。
pub const K3_ONLINEDEV_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r56'];var req=process.mainModule&&process.mainModule.require;if(typeof req!=='function'){return 'ERR:no require'}var electron=req('electron');var all=electron.webContents.getAllWebContents();var target=null;for(var i=0;i<all.length;i++){var u='';try{u=String(all[i].getURL())}catch(err){}if(u.indexOf('#/main/message')>=0){target=all[i];break}}if(!target){return 'ERR:main/message webContents not found'}if(st==='waiting'){G['__caligo_r56']='READ_DONE:'+String(G['__caligo_r56_tmp']||'').slice(0,64000);return G['__caligo_r56']}if(!st){var ev=electron.ipcMain._events['RM_IPCFROM_RENDERER2'];var list=Array.isArray(ev)?ev:[ev];var taps=G.__caligo_tap_fns||[];var real=null;for(var j=0;j<list.length;j++){var isTap=false;for(var k=0;k<taps.length;k++){if(list[j]===taps[k].fn){isTap=true;break}}if(!isTap){real=list[j];break}}if(!real){return 'ERR:no real handler'}var fakeEvent={sender:target,senderFrame:{url:'app://./renderer/index.html'},frameId:1,processId:5,frameTreeNodeId:2};function fire(cmdName,payload){var cid='od-'+cmdName+'-'+Date.now();real(fakeEvent,{type:'request',callbackId:cid,eventName:'ntApi',peerId:2},{cmdName:cmdName,cmdType:'invoke',payload:payload});return cid}function readResp(cid,waitMs,cb){var t0=Date.now();function poll(){var log=G.__caligo_wclog||[];for(var i=log.length-1;i>=0;i--){try{var a0=log[i].args[0];if(a0&&String(a0.j).indexOf(cid)>=0){cb({ch:log[i].ch,raw:String(log[i].args[1]?log[i].args[1].j:'').slice(0,1200)});return}}catch(e){}}if(Date.now()-t0>waitMs){cb(null);return}setTimeout(poll,300)}poll()}var log=[];var c1=fire('nodeIKernelMsgService/getOnlineDev',[]);readResp(c1,3000,function(r1){log.push({step:'getOnlineDev',r:r1});var c2=fire('nodeIKernelMsgService/getAutoReplyTextList',[]);readResp(c2,3000,function(r2){log.push({step:'getAutoReply',r:r2});var c3=fire('nodeIKernelMsgService/getMsgSetting',[]);readResp(c3,3000,function(r3){log.push({step:'getMsgSetting',r:r3});G['__caligo_r56_tmp']='LOG:'+JSON.stringify(log).slice(0,9000)})})});G['__caligo_r56']='waiting';return 'kicked'}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3 会话键 v2:Map 键('nt'/'gpro')而非值('nt_3')做 getNTWrapperSession 参数。
pub const K3_SESSIONKEY2_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var W=q.NodeIQQNTStartupSessionWrapper;var w=W.create();var ids=w.getSessionIdList();var keys=[];ids.forEach(function(v,k){keys.push({k:String(k),v:String(v)})});o.ids=keys;var cand=['nt','gpro'];for(var i=0;i<cand.length;i++){try{var ls=S.getNTWrapperSession(cand[i]);o[cand[i]]=typeof ls;if(ls&&typeof ls==='object'){try{o[cand[i]+'_sid']=String(ls.getSessionId()).slice(0,60)}catch(e1){o[cand[i]+'_sidErr']=String(e1).slice(0,120)}try{var ms=ls.getMsgService();o[cand[i]+'_msg']=typeof ms;o[cand[i]+'_send']=!!(ms&&ms.sendMsg)}catch(e2){o[cand[i]+'_msgErr']=String(e2).slice(0,150)}}}catch(e0){o[cand[i]+'Err']=String(e0).slice(0,150)}}try{var v1=ids.get('nt');o.vNt=String(v1).slice(0,40);var ls2=S.getNTWrapperSession(v1);o.byValNt=typeof ls2}catch(e3){o.valErr=String(e3).slice(0,150)}return JSON.stringify(o).slice(0,4000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 活会话服务面:getNTWrapperSession(当前 id)→ sid/MsgService/GroupService 全验证。
pub const K3_LIVESVC_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var W=q.NodeIQQNTStartupSessionWrapper;var w=W.create();var ids=w.getSessionIdList();var cur=String(ids.get('nt'));o.curId=cur;var ls=S.getNTWrapperSession(cur);o.type=typeof ls;if(ls&&typeof ls==='object'){try{o.sid=String(ls.getSessionId()).slice(0,80)}catch(e1){o.sidErr=String(e1).slice(0,150)}try{var ms=ls.getMsgService();o.msg=typeof ms;o.send=!!(ms&&ms.sendMsg);o.gets=!!(ms&&ms.getMsgs);o.listen=!!(ms&&ms.addKernelMsgListener)}catch(e2){o.msgErr=String(e2).slice(0,180)}try{var gs=ls.getGroupService();o.group=typeof gs;o.gList=!!(gs&&gs.getGroupList)}catch(e3){o.groupErr=String(e3).slice(0,180)}try{var bs=ls.getBuddyService();o.buddy=typeof bs}catch(e4){o.buddyErr=String(e4).slice(0,150)}try{var ps=ls.getProfileService();o.profile=typeof ps}catch(e5){o.profileErr=String(e5).slice(0,150)}try{o.proto=Object.getOwnPropertyNames(Object.getPrototypeOf(ls)).slice(0,50)}catch(e6){}}return JSON.stringify(o).slice(0,5000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 会话扫描:nt_0..nt_9 逐个验证 sid 与 getMsgService 有效性,找 App 活会话。
pub const K3_SESSIONSCAN_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;for(var i=0;i<10;i++){var id='nt_'+i;try{var ls=S.getNTWrapperSession(id);var e={type:typeof ls};if(ls&&typeof ls==='object'){try{e.sid=String(ls.getSessionId()).slice(0,60)}catch(e1){e.sidErr='x'}try{var ms=ls.getMsgService();e.msg=typeof ms;e.send=!!(ms&&ms.sendMsg)}catch(e2){e.msgErr='x'}}o[id]=e}catch(e0){o[id]={err:'thrown'}}}return JSON.stringify(o).slice(0,6000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,200)})}})()";

/// K3 NapCat 同款直发:活会话(nt_1).getMsgService() 上直接调用
/// getServerTime → generateMsgUniqueId → sendMsg('0',peer,elems,Map)。
/// peer.guildId=msgId;NapCat 官方形态;全 headless。
pub const K3_DIRECTSEND_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var ls=S.getNTWrapperSession('nt_1');if(!ls||typeof ls!=='object'){return JSON.stringify({err:'no live session'})}var ms=ls.getMsgService();if(!ms||!ms.sendMsg){return JSON.stringify({err:'no msg service'})}var msf=ls.getMSFService?ls.getMSFService():null;var sv=0;try{sv=msf.getServerTime()}catch(e1){o.svErr=String(e1).slice(0,150)}var mid=ms.generateMsgUniqueId(2,sv);o.mid=String(mid).slice(0,40);var peer={chatType:2,guildId:String(mid),peerUid:'263402786',peerUin:'263402786'};var elems=[{elementType:1,textElement:{content:'CALIGO-K3-GROUP-001'}}];try{var r=ms.sendMsg('0',peer,elems,new Map());o.sendType=typeof r;if(r&&typeof r.then==='function'){o.isPromise=true;r.then(function(res){try{globalThis.__caligo_directsend=JSON.stringify(res).slice(0,3000)}catch(e){globalThis.__caligo_directsend='res-strings-'+String(res).slice(0,1500)}},function(err){try{globalThis.__caligo_directsend='REJ:'+JSON.stringify(err).slice(0,2000)}catch(e){globalThis.__caligo_directsend='REJ-raw:'+String(err).slice(0,1500)}})}}catch(e2){o.sendErr=String(e2).slice(0,400)}return JSON.stringify(o).slice(0,3000)}catch(e){return JSON.stringify({fatal:String(e).slice(0,300)})}})()";

/// K3 直发结果读取:__caligo_directsend(Promise resolve 落点)。
pub const K3_DSREAD_SCRIPT: &str = "(function(){return JSON.stringify({r:globalThis.__caligo_directsend||null,set:('directsend' in globalThis)})})()";

/// K3-D1:活会话重扫 + addKernelMsgListener(捕获型,环 50×4KB),
/// listenerId 与事件环落 __caligo_r57/__caligo_recv。
pub const K3_RECV_ARM_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r57'];var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;if(st==='waiting'){G['__caligo_r57']='READ_DONE:'+JSON.stringify({events:G.__caligo_recv||[],lid:G.__caligo_lid||null}).slice(0,64000);return G['__caligo_r57']}if(!st){var live=null,lid=null;for(var i=0;i<10;i++){try{var ls=S.getNTWrapperSession('nt_'+i);if(ls&&typeof ls==='object'){var sid=ls.getSessionId();if(sid&&String(sid)!=='0'){var ms=ls.getMsgService();if(ms&&ms.sendMsg){live=ms;lid=String(sid);break}}}}catch(e){}}if(!live){return 'ERR:no live session'}if(G.__caligo_recv_lid){try{live.removeKernelMsgListener(G.__caligo_recv_lid)}catch(e){}}G.__caligo_recv=[];var L={onMsgInfoListUpdate:function(msgList){try{var arr=msgList&&msgList.msgList?msgList.msgList:msgList;var n=Array.isArray(arr)?arr.length:1;for(var i=0;i<(Array.isArray(arr)?arr.length:0);i++){var m=arr[i];var txt='';try{var els=m.elements||[];for(var j=0;j<els.length;j++){if(els[j]&&els[j].textElement){txt+=els[j].textElement.content||''}}}catch(e2){}G.__caligo_recv.push({msgId:String(m.msgId||'').slice(0,40),chatType:m.chatType,peerUid:String(m.peerUid||'').slice(0,40),senderUid:String(m.senderUid||'').slice(0,40),senderUin:String(m.senderUin||'').slice(0,20),sendStatus:m.sendStatus,msgType:m.msgType,textLen:txt.length,text:txt.slice(0,600)})}if(G.__caligo_recv.length>50){G.__caligo_recv.splice(0,G.__caligo_recv.length-50)}}catch(err){}},onRecvMsg:function(msgList){try{G.__caligo_recv.push({via:'onRecvMsg',n:msgList&&(msgList.msgList||msgList).length})}catch(err){}}};var lidRet=live.addKernelMsgListener(L);G.__caligo_recv_lid=lidRet;G.__caligo_lid=String(lidRet).slice(0,60);G.__caligo_livesid=lid;G.__caligo_r57='waiting';return 'ARMED sid='+lid+' lidRet='+String(lidRet).slice(0,60)}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-D1 v2:重武装——移除旧监听,onRecvMsg 直收数组并提取字段。
pub const K3_RECV_ARM2_SCRIPT: &str = "(function(){try{var G=globalThis;var st=G['__caligo_r58'];var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;if(st==='waiting'){G['__caligo_r58']='READ_DONE:'+JSON.stringify({events:G.__caligo_recv2||[],lid:G.__caligo_recv2_lid||null}).slice(0,64000);return G['__caligo_r58']}if(!st){var live=null,lid=null;for(var i=0;i<10;i++){try{var ls=S.getNTWrapperSession('nt_'+i);if(ls&&typeof ls==='object'){var sid=ls.getSessionId();if(sid&&String(sid)!=='0'){var ms=ls.getMsgService();if(ms&&ms.sendMsg){live=ms;lid=String(sid);break}}}}catch(e){}}if(!live){return 'ERR:no live session'}if(G.__caligo_recv2_lid){try{live.removeKernelMsgListener(G.__caligo_recv2_lid)}catch(e){}}if(G.__caligo_recv_lid){try{live.removeKernelMsgListener(G.__caligo_recv_lid)}catch(e){}}G.__caligo_recv2=[];var xtr=function(m){try{return {msgId:String(m.msgId||'').slice(0,40),chatType:m.chatType,peerUid:String(m.peerUid||'').slice(0,40),peerUin:String(m.peerUin||'').slice(0,20),senderUid:String(m.senderUid||'').slice(0,40),senderUin:String(m.senderUin||'').slice(0,20),sendStatus:m.sendStatus,msgType:m.msgType,subMsgType:m.subMsgType,txt:(function(){var t='';var els=m.elements||[];for(var j=0;j<els.length;j++){if(els[j]&&els[j].textElement){t+=els[j].textElement.content||''}}return t.slice(0,600)})()}}catch(err){return {xErr:String(err).slice(0,100)}}};var L={onRecvMsg:function(){try{var a=arguments[0];var arr=Array.isArray(a)?a:(a&&a.msgList?a.msgList:[a]);for(var i=0;i<arr.length;i++){G.__caligo_recv2.push(xtr(arr[i]))}if(G.__caligo_recv2.length>50){G.__caligo_recv2.splice(0,G.__caligo_recv2.length-50)}}catch(err){}},onMsgInfoListUpdate:function(){try{var a=arguments[0];var arr=Array.isArray(a)?a:(a&&a.msgList?a.msgList:[a]);for(var i=0;i<arr.length;i++){var e=xtr(arr[i]);e.via='update';G.__caligo_recv2.push(e)}if(G.__caligo_recv2.length>50){G.__caligo_recv2.splice(0,G.__caligo_recv2.length-50)}}catch(err){}},onActiveMsgListUpdate:function(k,msgList){try{var arr=Array.isArray(msgList)?msgList:(msgList&&msgList.msgList?msgList.msgList:[]);for(var i=0;i<arr.length;i++){var e=xtr(arr[i]);e.via='active';G.__caligo_recv2.push(e)}if(G.__caligo_recv2.length>50){G.__caligo_recv2.splice(0,G.__caligo_recv2.length-50)}}catch(err){}}};var lidRet=live.addKernelMsgListener(L);G.__caligo_recv2_lid=lidRet;G.__caligo_r58='waiting';return 'ARMED2 lidRet='+String(lidRet).slice(0,60)}return 'ERR:bad state'}catch(e){return 'ERR:'+String(e).slice(0,300)}})()";

/// K3-D 收官:C2C 发送(FRIEND-B,CALIGO-K3-SEND-001)+ 移除全部消息监听。
pub const K3_C2CSEND_SCRIPT: &str = "(function(){try{var o={};var q=process._linkedBinding('QQNT');var S=q.NodeIQQNTWrapperSession;var live=null,lid=null;for(var i=0;i<10;i++){try{var ls=S.getNTWrapperSession('nt_'+i);if(ls&&typeof ls==='object'){var sid=ls.getSessionId();if(sid&&String(sid)!=='0'){var ms=ls.getMsgService();if(ms&&ms.sendMsg){live=ms;lid=String(sid);break}}}}catch(e){}}if(!live){return JSON.stringify({err:'no live session'})}var sv=0;try{sv=live.getMSFService().getServerTime()}catch(e1){}var mid=live.generateMsgUniqueId(1,sv);var peer={chatType:1,guildId:String(mid),peerUid:'u_KxYzu_qQPbCC-Ca8mLteVA'};var elems=[{elementType:1,textElement:{content:'CALIGO-K3-SEND-001'}}];var r=live.sendMsg('0',peer,elems,new Map());o.mid=String(mid).slice(0,40);o.sendType=typeof r;if(r&&typeof r.then==='function'){r.then(function(res){try{globalThis.__caligo_c2c=JSON.stringify(res).slice(0,2000)}catch(e){globalThis.__caligo_c2c='ok'}})}try{var n1=0;if(globalThis.__caligo_recv_lid){live.removeKernelMsgListener(globalThis.__caligo_recv_lid);n1++}if(globalThis.__caligo_recv2_lid){live.removeKernelMsgListener(globalThis.__caligo_recv2_lid);n2++}}catch(e2){}o.listenersRemoved=true;return JSON.stringify(o).slice(0,1500)}catch(e){return JSON.stringify({fatal:String(e).slice(0,300)})}})()";
