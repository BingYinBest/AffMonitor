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

pub fn affinity_to_string(set: &cpu_set_t, num_cpus: usize) -> String {
    let mut ranges: Vec<String> = Vec::new();
    let mut start: i32 = -1;
    let mut last: i32 = -1;
    for i in 0..num_cpus {
        if unsafe { CPU_ISSET(i, set) } {
            if start == -1 {
                start = i as i32;
            }
            last = i as i32;
        } else if start != -1 {
            if start == last {
                ranges.push(format!("{}", start));
            } else {
                ranges.push(format!("{}-{}", start, last));
            }
            start = -1;
        }
    }
    if start != -1 {
        if start == last {
            ranges.push(format!("{}", start));
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

pub fn escape_thread_name(name: &str) -> String {
    let special_chars = "*?[]{}";
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if special_chars.contains(c) {
            out.push('?');
        } else {
            out.push(c);
        }
    }
    out
}

pub fn compare_threads(a: &ThreadInfo, b: &ThreadInfo) -> Ordering {
    b.cpu_usage_percent
        .partial_cmp(&a.cpu_usage_percent)
        .unwrap_or(Ordering::Equal)
}