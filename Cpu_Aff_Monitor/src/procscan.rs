use std::fs::{self, File};
use std::io::Read;
use crate::types::SystemContext;
use crate::utils::now_secs;

pub fn read_cmdline_name(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/cmdline", pid);
    let bytes = fs::read(&path).ok()?;
    // argv 以 \0 分隔；取第一个非空字段（argv[0]），避免后续 argv 残留干扰 basename
    let first = bytes.split(|&b| b == 0).find(|s| !s.is_empty())?;
    let base = first.rsplit(|&b| b == b'/').next().unwrap_or(first);
    let s = std::str::from_utf8(base).ok()?;
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn get_pid_by_package(ctx: &mut SystemContext, pkg: &str) -> Option<i32> {
    let current_time = now_secs();
    for i in 0..ctx.pid_cache.len() {
        if !ctx.pid_cache[i].pkg.is_empty()
            && ctx.pid_cache[i].pkg == pkg
            && current_time - ctx.pid_cache[i].last_update < 10
        {
            return Some(ctx.pid_cache[i].pid);
        }
    }

    let mut result: Option<i32> = None;
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let pid: i32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if let Some(proc_name) = read_cmdline_name(pid as u32) {
                if proc_name == pkg
                    || (proc_name.starts_with(pkg) && proc_name.as_bytes().get(pkg.len()) == Some(&b':'))
                {
                    result = Some(pid);
                    // 优先原地刷新同 pkg 的旧条目（含已过期条目），避免包重启后缓存残留旧 pid
                    let mut updated = false;
                    for j in 0..ctx.pid_cache.len() {
                        if ctx.pid_cache[j].pkg == pkg {
                            ctx.pid_cache[j].pid = pid;
                            ctx.pid_cache[j].last_update = current_time;
                            updated = true;
                            break;
                        }
                    }
                    if !updated {
                        for j in 0..ctx.pid_cache.len() {
                            if ctx.pid_cache[j].pkg.is_empty() {
                                ctx.pid_cache[j].pkg = pkg.to_string();
                                ctx.pid_cache[j].pid = pid;
                                ctx.pid_cache[j].last_update = current_time;
                                break;
                            }
                        }
                    }
                    break;
                }
            }
        }
    }
    result
}

pub fn can_read_proc_task() -> bool {
    let tid = unsafe { libc::gettid() };
    let path = format!("/proc/self/task/{}/comm", tid);
    if let Ok(mut f) = File::open(&path) {
        let mut buf = [0u8; 63];
        if let Ok(n) = f.read(&mut buf) {
            return n > 0;
        }
    }
    false
}

pub fn setup_permissions(ctx: &mut SystemContext) {
    if ctx.permissions_adjusted {
        return;
    }
    if can_read_proc_task() {
        ctx.permissions_adjusted = true;
        return;
    }
    let chmod_cmd = "chmod -R 775 /proc/*/task/*/comm /proc/*/task/*/stat /proc/*/task/*/status /proc/*/task/*/schedstat 2>/dev/null";
    if crate::cmd::system_cmd(chmod_cmd) == 0 && can_read_proc_task() {
        ctx.permissions_adjusted = true;
        return;
    }
    let mut selinux_state = [0u8; 16];
    if crate::cmd::exec_cmd("getenforce", &mut selinux_state).is_ok()
        && String::from_utf8_lossy(&selinux_state).contains("Enforcing")
    {
        ctx.original_selinux_state = 1;
        crate::cmd::system_cmd("setenforce 0");
        ctx.permissions_adjusted = can_read_proc_task();
    } else {
        ctx.original_selinux_state = 0;
        ctx.permissions_adjusted = true;
    }
}

pub fn restore_permissions(ctx: &SystemContext) {
    if ctx.original_selinux_state == 1 {
        crate::cmd::system_cmd("setenforce 1");
    }
    crate::cmd::system_cmd("chmod -R 555 /proc/*/task/*/comm /proc/*/task/*/stat /proc/*/task/*/status /proc/*/task/*/schedstat 2>/dev/null");
}

pub fn get_packages_from_config(
    config_file: &str,
) -> Result<Vec<(String, bool)>, ()> {
    let content = fs::read_to_string(config_file).map_err(|_| ())?;

    // 1. 先解析配置文件，得到目标包列表
    let mut wanted: Vec<(String, bool)> = Vec::new();
    for line in content.lines() {
        let mut pkg = line.trim();
        if pkg.is_empty() || pkg.starts_with('#') {
            continue;
        }
        let mut is_system = false;
        if let Some(eq) = pkg.find('=') {
            let tag = pkg[eq + 1..].trim();
            is_system = tag == "is_system";
            pkg = pkg[..eq].trim();
        }
        if !pkg.is_empty() {
            wanted.push((pkg.to_string(), is_system));
        }
    }
    if wanted.is_empty() {
        return Err(());
    }

    // 2. 一次扫描 /proc，把每个进程的 cmdline 与所有配置包匹配（O(N) 而非 O(N×K)）
    let mut result: Vec<(String, bool)> = Vec::new();
    let entries = fs::read_dir("/proc").map_err(|_| ())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let proc_name = match read_cmdline_name(pid) {
            Some(n) => n,
            None => continue,
        };
        for (pkg, is_sys_cfg) in &wanted {
            let matched = proc_name == *pkg
                || (proc_name.starts_with(pkg) && proc_name.as_bytes().get(pkg.len()) == Some(&b':'));
            if !matched {
                continue;
            }
            let is_sys = if proc_name.len() > pkg.len() && proc_name.as_bytes()[pkg.len()] == b':' {
                true
            } else {
                *is_sys_cfg
            };
            if !result.iter().any(|(n, _)| *n == proc_name) {
                result.push((proc_name.clone(), is_sys));
            }
            if result.len() >= crate::constants::MAX_PACKAGES {
                break;
            }
        }
        if result.len() >= crate::constants::MAX_PACKAGES {
            break;
        }
    }
    if result.is_empty() {
        Err(())
    } else {
        Ok(result)
    }
}