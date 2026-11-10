use std::ffi::CString;
use std::time::{Duration, Instant};
use libc::*;
use crate::constants::CMD_TIMEOUT_SECONDS;

/// 执行 shell 命令并把 stdout 读入 buf，最多 CMD_TIMEOUT_SECONDS 秒。
/// 子进程用 setpgid 自成进程组，超时/结束时 kill 整个进程组，
/// 从而一并回收 dumpsys|grep 这类孙进程（BUG-6 修复）。
pub fn exec_cmd(cmd: &str, buf: &mut [u8]) -> Result<usize, ()> {
    unsafe {
        let mut pipefd = [0i32; 2];
        if pipe(pipefd.as_mut_ptr()) == -1 {
            return Err(());
        }
        let pid = fork();
        if pid == -1 {
            close(pipefd[0]);
            close(pipefd[1]);
            return Err(());
        }
        if pid == 0 {
            // ---- 子进程 ----
            setpgid(0, 0); // 自成进程组，父进程可对整个组 kill
            close(pipefd[0]);
            dup2(pipefd[1], STDOUT_FILENO);
            close(pipefd[1]);
            // 恢复默认信号处置，避免继承父进程的 SIGTERM/SIGINT handler。
            let mut dfl: sigaction = std::mem::zeroed();
            dfl.sa_sigaction = 0; // SIG_DFL
            sigemptyset(&mut dfl.sa_mask);
            sigaction(SIGTERM, &dfl, std::ptr::null_mut());
            sigaction(SIGHUP, &dfl, std::ptr::null_mut());
            sigaction(SIGINT, &dfl, std::ptr::null_mut());
            sigaction(SIGALRM, &dfl, std::ptr::null_mut());
            // 双保险：子进程自身 5s 后被 SIGALRM 默认处置终止。
            alarm(CMD_TIMEOUT_SECONDS);
            let ccmd = match CString::new(cmd) {
                Ok(c) => c,
                Err(_) => _exit(127),
            };
            let sh = b"/system/bin/sh\0";
            let flag = b"sh\0";
            let cflag = b"-c\0";
            execl(
                sh.as_ptr() as *const c_char,
                flag.as_ptr() as *const c_char,
                cflag.as_ptr() as *const c_char,
                ccmd.as_ptr(),
                std::ptr::null::<libc::c_char>(),
            );
            _exit(127);
        }
        // ---- 父进程 ----
        close(pipefd[1]);
        // 把子进程拉入其进程组；若子进程已 exec 先跑，忽略 ESRCH。
        setpgid(pid, pid);

        let start = Instant::now();
        let total_timeout = Duration::from_secs(CMD_TIMEOUT_SECONDS as u64);
        let mut total = 0usize;
        loop {
            if total + 1 >= buf.len() {
                break;
            }
            let elapsed = start.elapsed();
            if elapsed >= total_timeout {
                break;
            }
            let remaining_ms = (total_timeout - elapsed).as_millis() as i32;
            let mut pfd = pollfd {
                fd: pipefd[0],
                events: POLLIN,
                revents: 0,
            };
            let pr = poll(&mut pfd, 1, remaining_ms);
            if pr < 0 {
                continue; // 被信号打断，继续轮询直到总超时
            }
            if pr == 0 {
                break; // 读超时
            }
            if (pfd.revents & (POLLIN | POLLHUP)) == 0 {
                break;
            }
            let n = read(
                pipefd[0],
                buf[total..].as_mut_ptr() as *mut c_void,
                buf.len() - total - 1,
            );
            if n <= 0 {
                break;
            }
            total += n as usize;
        }
        buf[total] = 0;

        // 杀整个进程组，确保 sh 及其子进程（dumpsys/grep）都被回收。
        kill(-pid, SIGKILL);
        let mut status = 0;
        waitpid(pid, &mut status, 0);
        close(pipefd[0]);

        if total > 0 {
            Ok(total)
        } else {
            Err(())
        }
    }
}

pub fn system_cmd(cmd: &str) -> i32 {
    match std::process::Command::new("sh").arg("-c").arg(cmd).status() {
        Ok(s) => s.code().unwrap_or(-1),
        Err(_) => -1,
    }
}
