use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub static KEEP_RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn signal_handler(_sig: libc::c_int) {
    KEEP_RUNNING.store(false, Ordering::SeqCst);
}

pub fn setup_signal_handlers() {
    unsafe {
        // 忽略 SIGPIPE：stdout 管道被对端关闭（如 | head）时不 panic，而是安全退出。
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);

        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGHUP, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    }
}

/// 分片睡眠，每 200ms 检查一次退出标志，使 SIGTERM/SIGINT 响应延迟 < 200ms。
/// 修复主循环 sleep 5-15s 不中断导致退出慢的问题。
pub fn sleep_fragmented(total: Duration) {
    const CHUNK_MS: u64 = 200;
    let mut remaining = total.as_millis();
    while remaining > 0 {
        if !KEEP_RUNNING.load(Ordering::SeqCst) {
            return;
        }
        let chunk = remaining.min(CHUNK_MS as u128) as u64;
        std::thread::sleep(Duration::from_millis(chunk));
        remaining = remaining.saturating_sub(chunk as u128);
    }
}
