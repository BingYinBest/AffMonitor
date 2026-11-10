use libc::{cpu_set_t, CPU_ISSET};
use crate::types::ThreadInfo;
use std::cmp::Ordering;

pub fn now_secs() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

pub fn mono_time() -> libc::timespec {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts
}

pub fn get_nprocs() -> usize {
    let r = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if r <= 0 {
        1
    } else {
        r as usize
    }
}

/// 把 cpu_set_t 转成 "0-3,6" 形式。按实际存在的 cpu_ids 遍历，
/// 兼容非连续 / 离线 CPU。
pub fn affinity_to_string(set: &cpu_set_t, cpu_ids: &[i32]) -> String {
    let mut ranges: Vec<String> = Vec::new();
    let mut start: i32 = -1;
    let mut last: i32 = -1;
    for &id in cpu_ids {
        if id < 0 {
            continue;
        }
        if unsafe { CPU_ISSET(id as usize, set) } {
            if start == -1 {
                start = id;
            }
            last = id;
        } else if start != -1 {
            if start == last {
                ranges.push(start.to_string());
            } else {
                ranges.push(format!("{}-{}", start, last));
            }
            start = -1;
        }
    }
    if start != -1 {
        if start == last {
            ranges.push(start.to_string());
        } else {
            ranges.push(format!("{}-{}", start, last));
        }
    }
    if ranges.is_empty() {
        "?".to_string()
    } else {
        ranges.join(",")
    }
}

/// 线程名转为可安全用于 AppOptR 线程模式的字面量。
/// AppOptR 线程模式是 fnmatch 模式（FNM_NOESCAPE，无转义）；`{}`/`=` 会破坏其行解析，
/// `*?[]` 会被当作通配导致过度匹配。含这些字符的名字无法精确表达，返回 None 跳过，
/// 避免生成错误规则。
pub fn appoptr_pattern(name: &str) -> Option<&str> {
    if name.is_empty() {
        return None;
    }
    if name.contains('{') || name.contains('}') || name.contains('=') {
        return None;
    }
    if name.contains('*') || name.contains('?') || name.contains('[') || name.contains(']') {
        return None;
    }
    Some(name)
}

pub fn compare_threads(a: &ThreadInfo, b: &ThreadInfo) -> Ordering {
    b.cpu_usage_percent
        .partial_cmp(&a.cpu_usage_percent)
        .unwrap_or(Ordering::Equal)
}
