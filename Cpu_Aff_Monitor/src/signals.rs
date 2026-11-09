use std::sync::atomic::{AtomicBool, Ordering};

pub static KEEP_RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn signal_handler(_sig: libc::c_int) {
    KEEP_RUNNING.store(false, Ordering::SeqCst);
}

pub fn setup_signal_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGHUP, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    }
}