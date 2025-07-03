use std::{cell::RefCell, ffi::CStr, sync::atomic::AtomicBool};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ThreadType {
    Unkown,
    MainThread,
}

thread_local!(static THREAD_TYPE: RefCell<ThreadType> = const { RefCell::new(ThreadType::Unkown) });

static MAIN_THREAD_IDENTIFIED: AtomicBool = AtomicBool::new(false);

/// Marks the current thread as the main thread for thread checking later.
pub fn mark_current_as_main() {
    THREAD_TYPE.replace(ThreadType::MainThread);
    MAIN_THREAD_IDENTIFIED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn ensure_main_thread(fn_name: &'static str) {
    if !is_thread_checking_enabled() {
        return;
    }

    if !is_main_thread() {
        log_misbehaviour(fn_name, "Called outside of main thread.");
    }
}

pub(crate) fn ensure_non_main_thread(fn_name: &'static str) {
    if !is_thread_checking_enabled() {
        return;
    }

    if is_main_thread() {
        log_misbehaviour(fn_name, "Called from main thread but should not have been.");
    }
}

#[no_mangle]
pub(crate) extern "C" fn ffi_ensure_main_thread(fn_name: *const std::ffi::c_char) {
    ensure_main_thread(unsafe { CStr::from_ptr(fn_name).to_str().unwrap() } );
}

#[no_mangle]
pub(crate) extern "C" fn ffi_ensure_non_main_thread(fn_name: *const std::ffi::c_char) {
    ensure_non_main_thread(unsafe { CStr::from_ptr(fn_name).to_str().unwrap() } );
}

pub(crate) fn is_thread_checking_enabled() -> bool {
    MAIN_THREAD_IDENTIFIED.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn is_main_thread() -> bool {
    assert!(is_thread_checking_enabled());
    THREAD_TYPE.with_borrow(|t| *t == ThreadType::MainThread)
}

fn log_misbehaviour(fn_name: &'static str, message: &'static str) {
    eprintln!("{}: {}", fn_name, message);
}
