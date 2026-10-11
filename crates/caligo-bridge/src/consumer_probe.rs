//! Bounded response-consumer callback capture for the research bridge.
//!
//! QQ callbacks only copy bounded diagnostic fields into preallocated slots.
//! They do not allocate, wait on locks, perform request matching, or write
//! files. The resident worker drains the slots and owns all formatting and IO.

use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::OnceLock;

const CTX_MAGIC: u64 = 0xC0A1_5E09_C0DE;
const CONTEXT_CAPACITY: usize = 16;
const RESPONSE_CAPACITY: usize = 256;
const MAX_ERR_BYTES: usize = 256;
const SLOT_FREE: u8 = 0;
const SLOT_WRITING: u8 = 1;
const SLOT_READY: u8 = 2;
const CONTEXT_ACTIVE: u8 = 1;
const CONTEXT_QUARANTINED: u8 = 2;
const EMPTY_MGR_RVA: u64 = 0x2338;
const EMPTY_INV_RVA: u64 = 0x236A;

/// Leaked, immutable, per-session context. Counters are atomic because a
/// callback may still arrive after its resident worker has begun shutdown.
#[repr(C)]
pub struct ConsumerCtx {
    pub report: [u16; 180],
    pub wrapper_base: u64,
    pub generation: u64,
    pub magic: u64,
    manager_calls: AtomicU64,
    callback_overflow: AtomicU64,
    lifecycle: AtomicU8,
    callbacks_in_flight: AtomicU64,
    late_callbacks_rejected: AtomicU64,
}

/// Marks one resident callback generation inactive when its owner stops.
/// Dropping this guard never frees its context: QQ may still invoke a delegate,
/// so the module and immutable context stay resident while late captures are
/// rejected before reading callback-owned diagnostic pointers.
pub(crate) struct GenerationGuard {
    generation: u64,
}

impl GenerationGuard {
    pub(crate) fn new(generation: u64) -> Self {
        Self { generation }
    }

    pub(crate) fn quarantine(&self) {
        quarantine_generation(self.generation);
    }
}

impl Drop for GenerationGuard {
    fn drop(&mut self) {
        self.quarantine();
    }
}

fn quarantine_generation(generation: u64) {
    if let Some(ctx) = context_for_generation(generation) {
        ctx.lifecycle.store(CONTEXT_QUARANTINED, Ordering::SeqCst);
        // Wait only for callbacks that already passed admission. New entries
        // recheck lifecycle after incrementing and cannot race past this fence.
        while ctx.callbacks_in_flight.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
    }
}

struct CallbackLease(&'static ConsumerCtx);

impl Drop for CallbackLease {
    fn drop(&mut self) {
        self.0.callbacks_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

fn enter_callback(ctx: &'static ConsumerCtx) -> Option<CallbackLease> {
    if ctx.lifecycle.load(Ordering::SeqCst) != CONTEXT_ACTIVE {
        ctx.late_callbacks_rejected.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    ctx.callbacks_in_flight.fetch_add(1, Ordering::SeqCst);
    if ctx.lifecycle.load(Ordering::SeqCst) != CONTEXT_ACTIVE {
        ctx.callbacks_in_flight.fetch_sub(1, Ordering::SeqCst);
        ctx.late_callbacks_rejected.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    Some(CallbackLease(ctx))
}

struct ContextSlot {
    state: AtomicU8,
    generation: AtomicU64,
    context: AtomicUsize,
}

impl ContextSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_FREE),
            generation: AtomicU64::new(0),
            context: AtomicUsize::new(0),
        }
    }
}

static CONTEXTS: [ContextSlot; CONTEXT_CAPACITY] = [const { ContextSlot::new() }; CONTEXT_CAPACITY];
static CONTEXT_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Copy)]
struct CallbackCapture {
    context: usize,
    cookie: [u8; 16],
    err_code: i32,
    err: [u8; MAX_ERR_BYTES],
    err_len: u16,
    err_flags: u8,
    span_shape: u8,
}

impl CallbackCapture {
    const EMPTY: Self = Self {
        context: 0,
        cookie: [0; 16],
        err_code: 0,
        err: [0; MAX_ERR_BYTES],
        err_len: 0,
        err_flags: 0,
        span_shape: 0,
    };
}

struct ResponseSlot {
    state: AtomicU8,
    capture: std::cell::UnsafeCell<CallbackCapture>,
}

impl ResponseSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(SLOT_FREE),
            capture: std::cell::UnsafeCell::new(CallbackCapture::EMPTY),
        }
    }
}

// The slot state CAS grants exclusive access to capture.
unsafe impl Sync for ResponseSlot {}

static RESPONSE_SLOTS: [ResponseSlot; RESPONSE_CAPACITY] =
    [const { ResponseSlot::new() }; RESPONSE_CAPACITY];
static WRITE_HINT: AtomicUsize = AtomicUsize::new(0);
static ORPHAN_CALLBACKS: AtomicU64 = AtomicU64::new(0);

/// Once a QQ delegate stores these Rust function pointers, Windows must keep
/// this DLL mapped until process exit. The QQ dispatcher has no proven
/// unsubscribe/drain contract, so stopping our worker cannot make unloading
/// safe.
fn pin_callback_module() -> Result<(), u32> {
    static RESULT: OnceLock<Result<(), u32>> = OnceLock::new();
    *RESULT.get_or_init(|| {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::LibraryLoader::{
            GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            GET_MODULE_HANDLE_EX_FLAG_PIN,
        };

        let mut module = core::ptr::null_mut();
        let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
        // With FROM_ADDRESS, this parameter is an address inside the bridge
        // image, not a string. PIN makes the loader retain the image until
        // process termination; the returned handle is intentionally unused.
        let pinned = unsafe {
            GetModuleHandleExW(
                flags,
                consumer_invoke as *const () as windows_sys::core::PCWSTR,
                &mut module,
            )
        };
        if pinned != 0 {
            Ok(())
        } else {
            Err(unsafe { GetLastError() })
        }
    })
}

/// Register one immutable generation context. A full registry is an explicit
/// admission failure; callers must stop before attaching or dispatching.
pub fn leak_ctx(
    wrapper_base: usize,
    report_path: *const u16,
    generation: u64,
) -> Option<&'static ConsumerCtx> {
    if report_path.is_null() {
        return None;
    }
    // This is the last common gate before a caller can build a delegate that
    // points into this module. Do not publish a context if the loader cannot
    // guarantee the callback code remains mapped for a late QQ invocation.
    pin_callback_module().ok()?;
    let _guard = CONTEXT_INIT_LOCK.lock().ok()?;
    if let Some(existing) = context_for_generation(generation) {
        return Some(existing);
    }
    let slot = CONTEXTS.iter().find(|slot| {
        slot.state
            .compare_exchange(SLOT_FREE, SLOT_WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    })?;
    let mut ctx = ConsumerCtx {
        report: [0; 180],
        wrapper_base: wrapper_base as u64,
        generation,
        magic: CTX_MAGIC,
        manager_calls: AtomicU64::new(0),
        callback_overflow: AtomicU64::new(0),
        lifecycle: AtomicU8::new(CONTEXT_ACTIVE),
        callbacks_in_flight: AtomicU64::new(0),
        late_callbacks_rejected: AtomicU64::new(0),
    };
    // SAFETY: caller owns a valid NUL-terminated UTF-16 path during init.
    unsafe {
        let mut i = 0usize;
        while i < 179 && *report_path.add(i) != 0 {
            ctx.report[i] = *report_path.add(i);
            i += 1;
        }
    }
    let ctx = Box::leak(Box::new(ctx));
    slot.generation.store(generation, Ordering::Relaxed);
    slot.context
        .store(ctx as *const ConsumerCtx as usize, Ordering::Relaxed);
    slot.state.store(SLOT_READY, Ordering::Release);
    Some(ctx)
}

fn context_for_generation(generation: u64) -> Option<&'static ConsumerCtx> {
    CONTEXTS.iter().find_map(|slot| {
        if slot.state.load(Ordering::Acquire) == SLOT_READY
            && slot.generation.load(Ordering::Relaxed) == generation
        {
            let ptr = slot.context.load(Ordering::Relaxed);
            if ptr != 0 {
                // SAFETY: contexts are leaked before the slot is published and
                // registry entries never change after SLOT_READY.
                return Some(unsafe { &*(ptr as *const ConsumerCtx) });
            }
        }
        None
    })
}

fn context_for_pointer(pointer: usize) -> Option<&'static ConsumerCtx> {
    CONTEXTS.iter().find_map(|slot| {
        (slot.state.load(Ordering::Acquire) == SLOT_READY
            && slot.context.load(Ordering::Relaxed) == pointer)
            .then(|| {
                // SAFETY: pointer equality matches a published leaked context.
                unsafe { &*(pointer as *const ConsumerCtx) }
            })
    })
}

fn context_from_cookie(lo: u64) -> Option<&'static ConsumerCtx> {
    context_for_pointer(lo as usize).or_else(|| context_for_generation(lo))
}

fn safe_read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: RPM reads from this process into a stack-owned qword.
    unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            (&mut out as *mut u64).cast(),
            core::mem::size_of::<u64>(),
            &mut got,
        )
    };
    (got == core::mem::size_of::<u64>()).then_some(out)
}

fn safe_read_into(addr: usize, out: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut got = 0usize;
    // SAFETY: RPM writes at most out.len() bytes to this stack-owned slice.
    unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            out.as_mut_ptr().cast(),
            out.len(),
            &mut got,
        )
    };
    got == out.len()
}

// err_flags: 0 empty, 1 complete, 2 over budget (omitted), 3 unreadable.
fn capture_error(addr: usize, out: &mut CallbackCapture) {
    if addr < 0x10000 {
        return;
    }
    let mut first = [0u8; 1];
    if !safe_read_into(addr, &mut first) {
        out.err_flags = 3;
        return;
    }
    if first[0] & 1 == 0 {
        let len = (first[0] >> 1) as usize;
        if len == 0 {
            return;
        }
        if len > MAX_ERR_BYTES {
            out.err_flags = 2;
            return;
        }
        if !safe_read_into(addr.saturating_add(1), &mut out.err[..len]) {
            out.err_flags = 3;
            return;
        }
        out.err_len = len as u16;
        out.err_flags = 1;
        return;
    }
    let size = match safe_read_qword(addr.saturating_add(8)) {
        Some(v) => usize::try_from(v).ok(),
        None => None,
    };
    let data = match safe_read_qword(addr.saturating_add(16)) {
        Some(v) => usize::try_from(v).ok(),
        None => None,
    };
    let (size, data) = match (size, data) {
        (Some(size), Some(data)) => (size, data),
        _ => {
            out.err_flags = 3;
            return;
        }
    };
    if size == 0 {
        return;
    }
    if size > MAX_ERR_BYTES {
        out.err_flags = 2;
        return;
    }
    if data < 0x10000 || !safe_read_into(data, &mut out.err[..size]) {
        out.err_flags = 3;
        return;
    }
    out.err_len = size as u16;
    out.err_flags = 1;
}

// 0 unknown, 1 valid empty, 2 valid bounded span, 3 malformed/unreadable,
// 4 valid but too large. Span body bytes are deliberately never dereferenced.
fn capture_span_shape(addr: usize) -> u8 {
    if addr < 0x10000 {
        return 0;
    }
    let begin = match safe_read_qword(addr) {
        Some(v) => v,
        None => return 3,
    };
    let end = match safe_read_qword(addr.saturating_add(8)) {
        Some(v) => v,
        None => return 3,
    };
    let _cap_end = match safe_read_qword(addr.saturating_add(16)) {
        Some(v) => v,
        None => return 3,
    };
    if begin == 0 && end == 0 {
        return 1;
    }
    if end < begin {
        return 3;
    }
    let len = end - begin;
    if len == 0 {
        1
    } else if len <= 512 && begin > 0x10000 {
        2
    } else {
        4
    }
}

fn enqueue_capture(mut capture: CallbackCapture, context: Option<&'static ConsumerCtx>) {
    let start = WRITE_HINT.fetch_add(1, Ordering::Relaxed) % RESPONSE_CAPACITY;
    for offset in 0..RESPONSE_CAPACITY {
        let slot = &RESPONSE_SLOTS[(start + offset) % RESPONSE_CAPACITY];
        if slot
            .state
            .compare_exchange(
                SLOT_FREE,
                SLOT_WRITING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            if let Some(ctx) = context {
                capture.context = ctx as *const ConsumerCtx as usize;
            }
            // SAFETY: SLOT_WRITING is exclusively owned by this producer.
            unsafe { *slot.capture.get() = capture };
            slot.state.store(SLOT_READY, Ordering::Release);
            return;
        }
    }
    if let Some(ctx) = context {
        ctx.callback_overflow.fetch_add(1, Ordering::Relaxed);
    } else {
        ORPHAN_CALLBACKS.fetch_add(1, Ordering::Relaxed);
    }
}

fn with_ffi_panic_boundary(f: impl FnOnce()) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
}

/// QQ delegate manager with the statically verified op 0/1/3/4 contract.
/// No formatting or IO happens on this callback thread.
///
/// # Safety
/// `dest` and `out` must satisfy the profiled QQ delegate ABI contract.
pub unsafe extern "system" fn consumer_manager(
    dest: *mut c_void,
    op: u32,
    src: *mut c_void,
    _size: usize,
    out: *mut c_void,
) {
    with_ffi_panic_boundary(|| {
        let context = if !src.is_null() {
            let lo = safe_read_qword(src as usize).unwrap_or(0);
            context_from_cookie(lo)
        } else {
            None
        };
        if let Some(ctx) = context {
            ctx.manager_calls.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: pointers are supplied by the profiled QQ delegate contract.
        unsafe {
            match op {
                0 => {
                    if !out.is_null() && !src.is_null() {
                        let src_addr = src as usize;
                        let v0 = safe_read_qword(src_addr).unwrap_or(0);
                        let v1 = safe_read_qword(src_addr.saturating_add(8)).unwrap_or(0);
                        core::ptr::write(out as *mut u64, v0);
                        core::ptr::write((out as *mut u64).add(1), v1);
                    }
                    if !dest.is_null() {
                        core::ptr::write(
                            dest as *mut u64,
                            consumer_manager as *const () as usize as u64,
                        );
                        core::ptr::write(
                            (dest as *mut u64).add(1),
                            consumer_invoke as *const () as usize as u64,
                        );
                    }
                }
                1 | 3 => {
                    if !dest.is_null() {
                        if let Some(ctx) = context.filter(|c| c.wrapper_base > 0x10000) {
                            core::ptr::write(dest as *mut u64, ctx.wrapper_base + EMPTY_MGR_RVA);
                            core::ptr::write(
                                (dest as *mut u64).add(1),
                                ctx.wrapper_base + EMPTY_INV_RVA,
                            );
                        } else {
                            core::ptr::write(
                                dest as *mut u64,
                                consumer_manager as *const () as usize as u64,
                            );
                            core::ptr::write(
                                (dest as *mut u64).add(1),
                                consumer_invoke as *const () as usize as u64,
                            );
                        }
                    }
                }
                4 => {
                    if !out.is_null() {
                        core::ptr::write(out as *mut u64, 0);
                    }
                }
                _ => {}
            }
        }
    });
}

/// Response callback. It captures the per-send cookie and bounded diagnostics;
/// the worker resolves the generation and performs request correlation.
///
/// # Safety
/// `this`, `err_str`, and `extra` must be readable under the profiled callback
/// contract. RPM protects ordinary bad reads but cannot catch every SEH fault.
pub unsafe extern "system" fn consumer_invoke(
    this: *mut c_void,
    _tag: u32,
    err_code: i32,
    err_str: *const c_void,
    _writer: *const c_void,
    extra: *const c_void,
) {
    with_ffi_panic_boundary(|| {
        if this.is_null() {
            ORPHAN_CALLBACKS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let addr = this as usize;
        let lo = safe_read_qword(addr).unwrap_or(0);
        let hi = safe_read_qword(addr.saturating_add(8)).unwrap_or(0);
        let context = match context_from_cookie(lo) {
            Some(context) => context,
            None => {
                // A response without a registered generation cannot be
                // correlated safely. Audit it without consuming a shared slot.
                ORPHAN_CALLBACKS.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        let _callback_lease = enter_callback(context);
        if _callback_lease.is_none() {
            return;
        }
        let is_context_delegate = context_for_pointer(lo as usize).is_some();
        let has_cookie = !is_context_delegate && (lo != 0 || hi != 0);
        if !has_cookie {
            return;
        }
        let mut capture = CallbackCapture::EMPTY;
        capture.cookie[..8].copy_from_slice(&lo.to_le_bytes());
        capture.cookie[8..].copy_from_slice(&hi.to_le_bytes());
        capture.err_code = err_code;
        capture_error(err_str as usize, &mut capture);
        capture.span_shape = capture_span_shape(extra as usize);
        enqueue_capture(capture, Some(context));
    });
}

fn append_line(ctx: &ConsumerCtx, line: &str) {
    use std::io::Write;
    use std::os::windows::ffi::OsStringExt;
    let end = ctx
        .report
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(ctx.report.len());
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&ctx.report[..end]));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")
        });
}

/// Research-only static delegate used by g3_send_run.
pub fn build_cb(_wrapper_base: usize, ctx: &ConsumerCtx) -> [u64; 4] {
    [
        ctx as *const ConsumerCtx as u64,
        1,
        consumer_manager as *const () as usize as u64,
        consumer_invoke as *const () as usize as u64,
    ]
}

/// Register this resident generation. Failure is a hard startup failure.
pub fn init(wrapper_base: usize, report_path: *const u16, generation: u64) -> bool {
    leak_ctx(wrapper_base, report_path, generation).is_some()
}

/// Per-send delegate: 16-byte cookie followed by manager/invoker pointers.
pub fn build_cb_for_cookie(cookie: &crate::native_response::Cookie) -> [u64; 4] {
    [
        u64::from_le_bytes(cookie.0[..8].try_into().expect("fixed cookie width")),
        u64::from_le_bytes(cookie.0[8..].try_into().expect("fixed cookie width")),
        consumer_manager as *const () as usize as u64,
        consumer_invoke as *const () as usize as u64,
    ]
}

/// Drain bounded callback slots on the worker thread, then return complete
/// diagnostic strings for the existing response ledger interface.
pub fn drain_responses() -> Vec<(crate::native_response::Cookie, i32, String)> {
    let mut responses = Vec::new();
    for slot in &RESPONSE_SLOTS {
        if slot
            .state
            .compare_exchange(
                SLOT_READY,
                SLOT_WRITING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            continue;
        }
        // SAFETY: SLOT_WRITING is exclusively owned by this consumer.
        let capture = unsafe { *slot.capture.get() };
        slot.state.store(SLOT_FREE, Ordering::Release);
        let context = context_for_pointer(capture.context);
        let err = match capture.err_flags {
            1 => String::from_utf8_lossy(&capture.err[..capture.err_len as usize]).into_owned(),
            2 => "<diagnostic omitted: over budget>".to_owned(),
            3 => "<diagnostic unreadable>".to_owned(),
            _ => String::new(),
        };
        if let Some(ctx) = context {
            append_line(
                ctx,
                &format!(
                    "{{\"kind\":\"consumer_result\",\"generation\":{},\"err_code\":{},\"err\":{:?},\"span_shape\":{}}}",
                    ctx.generation, capture.err_code, err, capture.span_shape
                ),
            );
        }
        responses.push((
            crate::native_response::Cookie(capture.cookie),
            capture.err_code,
            err,
        ));
    }
    for slot in &CONTEXTS {
        if slot.state.load(Ordering::Acquire) != SLOT_READY {
            continue;
        }
        if let Some(ctx) = context_for_generation(slot.generation.load(Ordering::Relaxed)) {
            let manager_calls = ctx.manager_calls.swap(0, Ordering::Relaxed);
            let overflow = ctx.callback_overflow.swap(0, Ordering::Relaxed);
            let late_callbacks = ctx.late_callbacks_rejected.swap(0, Ordering::Relaxed);
            if manager_calls != 0 || overflow != 0 || late_callbacks != 0 {
                append_line(
                    ctx,
                    &format!(
                        "{{\"kind\":\"consumer_worker_summary\",\"generation\":{},\"manager_calls\":{},\"callback_overflow\":{},\"late_callbacks_rejected\":{}}}",
                        ctx.generation, manager_calls, overflow, late_callbacks
                    ),
                );
            }
        }
    }
    let orphaned = ORPHAN_CALLBACKS.swap(0, Ordering::Relaxed);
    if orphaned != 0 {
        if let Some(ctx) = CONTEXTS
            .iter()
            .find(|slot| slot.state.load(Ordering::Acquire) == SLOT_READY)
            .and_then(|slot| context_for_generation(slot.generation.load(Ordering::Relaxed)))
        {
            append_line(
                ctx,
                &format!("{{\"kind\":\"consumer_orphan_callback\",\"count\":{orphaned}}}"),
            );
        }
    }
    responses
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0xCA11_0000);
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn context() -> (&'static ConsumerCtx, std::path::PathBuf, Vec<u16>) {
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let report = std::env::temp_dir().join(format!("caligo-cp-{generation:x}.jsonl"));
        let _ = std::fs::remove_file(&report);
        let wide: Vec<u16> = report
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        let ctx = leak_ctx(0x180000000, wide.as_ptr(), generation).expect("context slot available");
        (ctx, report, wide)
    }

    fn short_string(s: &[u8]) -> [u8; 24] {
        let mut raw = [0u8; 24];
        raw[0] = (s.len() as u8) << 1;
        raw[1..1 + s.len()].copy_from_slice(s);
        raw
    }

    #[test]
    fn cloned_delegate_captures_cookie_and_worker_owns_io() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (ctx, report, _wide) = context();
        let cookie = crate::native_response::Cookie([
            (ctx.generation & 0xff) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0x88,
            0x77,
            0x66,
            0x55,
            0x44,
            0x33,
            0x22,
            0x11,
        ]);
        let mut cookie = cookie;
        cookie.0[..8].copy_from_slice(&ctx.generation.to_le_bytes());
        let src = build_cb_for_cookie(&cookie);
        let mut dest = [0u64; 4];
        // SAFETY: buffers reproduce the statically profiled delegate layout.
        unsafe {
            consumer_manager(
                dest.as_mut_ptr().add(2).cast(),
                0,
                src.as_ptr().cast_mut().cast(),
                0x10,
                dest.as_mut_ptr().cast(),
            );
        }
        assert_eq!(dest[0..2], src[0..2]);
        assert_eq!(dest[2], consumer_manager as *const () as usize as u64);
        assert_eq!(dest[3], consumer_invoke as *const () as usize as u64);
        let mut gate = [99u64; 2];
        // SAFETY: op=4 writes the profiled 8-byte empty-query result.
        unsafe {
            consumer_manager(
                core::ptr::null_mut(),
                4,
                core::ptr::null_mut(),
                0,
                gate.as_mut_ptr().cast(),
            );
        }
        assert_eq!(gate[0], 0, "op 4 zero means callable");
        let mut reset = [0u64; 2];
        // SAFETY: op=3 receives the source clone and destination delegate slots.
        unsafe {
            consumer_manager(
                reset.as_mut_ptr().cast(),
                3,
                dest.as_mut_ptr().cast(),
                0x10,
                core::ptr::null_mut(),
            );
        }
        assert_eq!(
            reset,
            [
                ctx.wrapper_base + EMPTY_MGR_RVA,
                ctx.wrapper_base + EMPTY_INV_RVA
            ]
        );
        let err = short_string(b"ok");
        let empty_span = [0u64; 3];
        // SAFETY: callback inputs are local, readable buffers.
        unsafe {
            consumer_invoke(
                dest.as_mut_ptr().cast(),
                0x10,
                0,
                err.as_ptr().cast(),
                core::ptr::null(),
                empty_span.as_ptr().cast(),
            );
        }
        assert!(!report.exists(), "callback thread must not write reports");
        let drained = drain_responses();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, cookie);
        assert_eq!(drained[0].1, 0);
        assert_eq!(drained[0].2, "ok");
        let output = std::fs::read_to_string(report).unwrap();
        assert!(output.contains("\"span_shape\":1"), "empty span is valid");
    }

    #[test]
    fn old_generation_context_remains_available_for_late_callback() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (old, old_report, _old_wide) = context();
        let (new, _new_report, _new_wide) = context();
        let mut cookie = crate::native_response::Cookie([0; 16]);
        cookie.0[..8].copy_from_slice(&old.generation.to_le_bytes());
        cookie.0[8..].copy_from_slice(&7u64.to_le_bytes());
        let cb = build_cb_for_cookie(&cookie);
        let err = short_string(b"late");
        // SAFETY: callback input storage is live for the duration of the call.
        unsafe {
            consumer_invoke(
                cb.as_ptr().cast_mut().cast(),
                0x10,
                0,
                err.as_ptr().cast(),
                core::ptr::null(),
                core::ptr::null(),
            );
        }
        assert_ne!(old.generation, new.generation);
        assert!(!old_report.exists());
        let drained = drain_responses();
        assert_eq!(drained.last().unwrap().0, cookie);
        assert!(
            old_report.exists(),
            "late callback is audited to old context"
        );
    }

    #[test]
    fn full_response_slots_count_overflow_without_overwriting_ready_entries() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (ctx, _report, _wide) = context();
        let mut cookie = crate::native_response::Cookie([0; 16]);
        cookie.0[..8].copy_from_slice(&ctx.generation.to_le_bytes());
        let cb = build_cb_for_cookie(&cookie);
        let err = short_string(b"x");
        let span = [0u64; 3];
        for _ in 0..=RESPONSE_CAPACITY {
            // SAFETY: callback inputs point to live stack buffers.
            unsafe {
                consumer_invoke(
                    cb.as_ptr().cast_mut().cast(),
                    0x10,
                    0,
                    err.as_ptr().cast(),
                    core::ptr::null(),
                    span.as_ptr().cast(),
                );
            }
        }
        let drained = drain_responses();
        assert_eq!(drained.len(), RESPONSE_CAPACITY);
        assert_eq!(ctx.callback_overflow.load(Ordering::Relaxed), 0);
        // drain_responses emits and resets the overflow diagnostic on worker.
        assert!(std::fs::read_to_string(_report)
            .unwrap()
            .contains("\"callback_overflow\":1"));
    }

    #[test]
    fn quarantined_generation_rejects_late_callback_before_reading_diagnostics() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (ctx, report, _wide) = context();
        let mut cookie = crate::native_response::Cookie([0; 16]);
        cookie.0[..8].copy_from_slice(&ctx.generation.to_le_bytes());
        cookie.0[8..].copy_from_slice(&7u64.to_le_bytes());
        let delegate = build_cb_for_cookie(&cookie);
        let err = short_string(b"before-stop");
        let span = [0u64; 3];

        // An active generation still captures its response normally.
        unsafe {
            consumer_invoke(
                delegate.as_ptr().cast_mut().cast(),
                0x10,
                0,
                err.as_ptr().cast(),
                core::ptr::null(),
                span.as_ptr().cast(),
            );
        }
        assert_eq!(drain_responses().len(), 1);

        let guard = GenerationGuard::new(ctx.generation);
        guard.quarantine();
        // Invalid diagnostic pointers prove the quarantine check returns
        // before touching callback-owned error/span memory.
        unsafe {
            consumer_invoke(
                delegate.as_ptr().cast_mut().cast(),
                0x10,
                0,
                1usize as *const c_void,
                core::ptr::null(),
                1usize as *const c_void,
            );
        }
        assert!(drain_responses().is_empty());
        let log = std::fs::read_to_string(report).unwrap();
        assert!(log.contains("\"late_callbacks_rejected\":1"));
    }

    #[test]
    fn generation_quarantine_waits_for_an_admitted_callback() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (ctx, _report, _wide) = context();
        let lease = enter_callback(ctx).expect("active generation admits callback");
        let generation = ctx.generation;
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let quarantine = std::thread::spawn(move || {
            quarantine_generation(generation);
            let _ = done_tx.send(());
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while ctx.lifecycle.load(Ordering::SeqCst) != CONTEXT_QUARANTINED
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(ctx.lifecycle.load(Ordering::SeqCst), CONTEXT_QUARANTINED);
        assert!(matches!(
            done_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        drop(lease);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("quarantine completes after the callback lease exits");
        quarantine.join().unwrap();
    }

    #[test]
    fn callback_without_registered_generation_is_audited_without_slot_admission() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (_ctx, _report, _wide) = context();
        let unknown = [u64::MAX, 7, 0, 0];
        let orphaned_before = ORPHAN_CALLBACKS.load(Ordering::Relaxed);
        // Unknown callback identity returns before error/span pointers are read.
        unsafe {
            consumer_invoke(
                unknown.as_ptr().cast_mut().cast(),
                0x10,
                0,
                1usize as *const c_void,
                core::ptr::null(),
                1usize as *const c_void,
            );
        }
        assert_eq!(
            ORPHAN_CALLBACKS.load(Ordering::Relaxed),
            orphaned_before + 1
        );
        assert!(drain_responses().is_empty());
    }

    #[test]
    fn simultaneous_generations_keep_callbacks_in_their_own_context() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (left, left_report, _left_wide) = context();
        let (right, right_report, _right_wide) = context();
        let make_cookie = |generation: u64, suffix: u64| {
            let mut cookie = crate::native_response::Cookie([0; 16]);
            cookie.0[..8].copy_from_slice(&generation.to_le_bytes());
            cookie.0[8..].copy_from_slice(&suffix.to_le_bytes());
            cookie
        };
        let workers: Vec<_> = [(left.generation, 1000u64), (right.generation, 2000u64)]
            .into_iter()
            .map(|(generation, base)| {
                std::thread::spawn(move || {
                    let cookie = make_cookie(generation, base);
                    let delegate = build_cb_for_cookie(&cookie);
                    let err = short_string(b"parallel");
                    let span = [0u64; 3];
                    for n in 0..100u64 {
                        let mut per_call = delegate;
                        per_call[1] = base + n;
                        // SAFETY: callback inputs are live local test buffers.
                        unsafe {
                            consumer_invoke(
                                per_call.as_mut_ptr().cast(),
                                0x10,
                                0,
                                err.as_ptr().cast(),
                                core::ptr::null(),
                                span.as_ptr().cast(),
                            );
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(
            !left_report.exists() && !right_report.exists(),
            "callback workers perform no file IO"
        );
        let drained = drain_responses();
        let left_count = drained
            .iter()
            .filter(|(cookie, _, _)| {
                u64::from_le_bytes(cookie.0[..8].try_into().unwrap()) == left.generation
            })
            .count();
        let right_count = drained
            .iter()
            .filter(|(cookie, _, _)| {
                u64::from_le_bytes(cookie.0[..8].try_into().unwrap()) == right.generation
            })
            .count();
        assert_eq!((left_count, right_count), (100, 100));
        let left_log = std::fs::read_to_string(left_report).unwrap();
        let right_log = std::fs::read_to_string(right_report).unwrap();
        assert!(left_log.contains(&format!("\"generation\":{}", left.generation)));
        assert!(right_log.contains(&format!("\"generation\":{}", right.generation)));
    }
}
