use std::fs::{self, File};
use std::io::Read;
use std::collections::HashSet;
use crate::types::SystemContext;
use crate::utils::now_secs;

/// 记录 SELinux 原始状态的标记文件，用于进程被 SIGKILL/崩溃后恢复。
const SELINUX_MARKER: &str = "/data/local/tmp/.moniguard_selinux";

pub fn can_read_path(path: &str) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut buf = [0u8; 16];
        return f.read(&mut buf).map(|n| n > 0).unwrap_or(false);
    }
    false
}

pub fn read_cmdline_name(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/cmdline", pid);
    let bytes = fs::read(&path).ok()?;
    // argv 以 \0 分隔；取第一个非空字段（argv[0]），避免后续 argv 残留干扰 basename。
    let first = bytes.split(|&b| b == 0).find(|s| !s.is_empty())?;
    let base = first.rsplit(|&b| b == b'/').next().unwrap_or(first);
    let s = std::str::from_utf8(base).ok()?;
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn write_selinux_marker(state: &str) {
    let _ = fs::write(SELINUX_MARKER, state);
}

fn clear_selinux_marker() {
    let _ = fs::remove_file(SELINUX_MARKER);
}

/// 启动时调用：若上次进程异常退出（SIGKILL/panic/OOM）导致 SELinux 残留 Permissive，
/// 这里恢复并清除标记。修复 BUG-4。
pub fn recover_selinux_on_startup() {
    if let Ok(state) = fs::read_to_string(SELINUX_MARKER) {
        let state = state.trim();
        if state == "Enforcing" {
            crate::cmd::system_cmd("setenforce 1");
        }
        clear_selinux_marker();
    }
}

pub fn get_pid_by_package(ctx: &mut SystemContext, pkg: &str) -> Option<i32> {
    let current_time = now_secs();
    if let Some(cache) = ctx.pid_cache.get(pkg) {
        if current_time - cache.last_update < 10 {
            return Some(cache.pid);
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
                    // 刷新缓存（含包重启后的旧 pid 覆盖）。
                    ctx.pid_cache.insert(pkg.to_string(), crate::types::PidCache {
                        pid,
                        last_update: current_time,
                    });
                    break;
                }
            }
        }
    }
    result
}

/// 针对目标线程路径的权限初始化：chmod 兜底，仍不可读则降级 SELinux（并记标记）。
pub fn setup_permissions(ctx: &mut SystemContext, probe_path: &str) {
    if ctx.permissions_adjusted {
        return;
    }
    if can_read_path(probe_path) {
        ctx.permissions_adjusted = true;
        return;
    }
    let chmod_cmd = "chmod -R 775 /proc/*/task/*/comm /proc/*/task/*/stat /proc/*/task/*/status /proc/*/task/*/schedstat 2>/dev/null";
    if crate::cmd::system_cmd(chmod_cmd) == 0 && can_read_path(probe_path) {
        ctx.permissions_adjusted = true;
        return;
    }
    let mut selinux_state = [0u8; 16];
    if crate::cmd::exec_cmd("getenforce", &mut selinux_state).is_ok()
        && String::from_utf8_lossy(&selinux_state).contains("Enforcing")
    {
        ctx.original_selinux_state = 1;
        // 记录原始状态，供崩溃后恢复（BUG-4）。
        write_selinux_marker("Enforcing");
        crate::cmd::system_cmd("setenforce 0");
        ctx.permissions_adjusted = can_read_path(probe_path);
    } else {
        ctx.original_selinux_state = 0;
        ctx.permissions_adjusted = true;
    }
}

pub fn restore_permissions(ctx: &SystemContext) {
    if ctx.original_selinux_state == 1 {
        crate::cmd::system_cmd("setenforce 1");
        clear_selinux_marker();
    }
    crate::cmd::system_cmd("chmod -R 555 /proc/*/task/*/comm /proc/*/task/*/stat /proc/*/task/*/status /proc/*/task/*/schedstat 2>/dev/null");
}

pub fn get_packages_from_config(config_file: &str) -> Result<Vec<(String, bool)>, ()> {
    let content = fs::read_to_string(config_file).map_err(|_| ())?;

    // 1. 解析配置文件，得到目标包列表。
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

    // 2. 一次扫描 /proc，匹配所有配置包，用 HashSet 去重（修复 O(n²)）。
    let mut result: Vec<(String, bool)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
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
            if seen.insert(proc_name.clone()) {
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
