p = 'crates/caligo-core/tests/daemon_client_lab.rs'
s = open(p, encoding='utf-8').read()
old = (
    "        if let CoreToControlMsg::QueryResult { state, native_id, .. } =\n"
    "            control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: \"R1\".into() }).unwrap()\n"
    "        {"
)
new = (
    "        let q = control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: \"R1\".into() }).unwrap();\n"
    "        if let CoreToControlMsg::QueryResult { state, native_id, .. } = q {\n"
    "            eprintln!(\"[dbg] query: state={state:?} native_id={native_id:?}\");"
)
assert old in s, 'query block not found'
s = s.replace(old, new)
open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('dbg added')
