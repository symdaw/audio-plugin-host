use std::{cell::RefCell, sync::atomic::AtomicBool};

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
    if !MAIN_THREAD_IDENTIFIED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    if is_main_thread() {
        log_misbehaviour(fn_name, "Called outside of main thread.");
    }
}

pub(crate) fn ensure_non_main_thread(fn_name: &'static str) {
    if !MAIN_THREAD_IDENTIFIED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    if !is_main_thread() {
        log_misbehaviour(fn_name, "Called from main thread but should not have been.");
    }
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
