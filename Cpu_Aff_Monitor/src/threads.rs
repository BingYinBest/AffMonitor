use std::fs::{self, File};
use std::io::Read;
use libc::{cpu_set_t, CPU_ZERO, sched_getaffinity};
use crate::constants::MAX_THREAD_LEN;
use crate::types::{PackageThreads, SystemContext, ThreadData, ThreadInfo};
use crate::utils::mono_time;

pub fn scan_thread_ids(pid: i32) -> Option<Vec<i32>> {
    let task_path = format!("/proc/{}/task", pid);
    let entries = fs::read_dir(&task_path).ok()?;
    let mut tids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(tid) = name.parse::<i32>() {
                tids.push(tid);
            }
        }
    }
    Some(tids)
}

fn read_u64(path: &str) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut buf = [0u8; 63];
    let n = f.read(&mut buf).ok()?;
    let s = std::str::from_utf8(&buf[..n]).ok()?;
    s.trim().parse::<u64>().ok()
}

pub fn read_thread_info(
    ctx: &mut SystemContext,
    pid: i32,
    tid: i32,
    pkg_idx: usize,
    curr_time: libc::timespec,
) -> Option<ThreadInfo> {
    let stat_path = format!("/proc/{}/task/{}/stat", pid, tid);
    let sched_path = format!("/proc/{}/task/{}/schedstat", pid, tid);
    let comm_path = format!("/proc/{}/task/{}/comm", pid, tid);

    let mut cpu_time = read_u64(&sched_path).unwrap_or(0);

    if cpu_time == 0 {
        if let Ok(mut f) = File::open(&stat_path) {
            let mut buf = [0u8; 256];
            if let Ok(n) = f.read(&mut buf) {
                let s = String::from_utf8_lossy(&buf[..n]);
                if let Some(rparen) = s.rfind(')') {
                    let after = &s[rparen + 1..];
                    let fields: Vec<&str> = after.split_whitespace().collect();
                    if fields.len() > 12 {
                        if let (Ok(ut), Ok(st)) = (fields[11].parse::<u64>(), fields[12].parse::<u64>()) {
                            let clk = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                            if clk > 0 {
                                cpu_time = (ut + st) * 1_000_000_000u64 / (clk as u64);
                            }
                        }
                    }
                }
            }
        } else {
            return None;
        }
    }

    let mut thread = ThreadInfo::new();
    thread.cpu_time = cpu_time;
    thread.tid = tid;

    if ctx.affinity_initialized && pkg_idx < ctx.last_thread_data.len() {
        for data in ctx.last_thread_data[pkg_idx].iter() {
            if data.pid == pid && data.tid == tid {
                // 先按 i64 计算（处理 tv_nsec 借位），再转 u64，避免直接 as u64 下溢
                let delta_sec = (curr_time.tv_sec - data.last_time.tv_sec) as i64;
                let delta_nsec = (curr_time.tv_nsec - data.last_time.tv_nsec) as i64;
                let delta_time_ns_i = delta_sec * 1_000_000_000i64 + delta_nsec;
                let delta_time_ns = if delta_time_ns_i > 0 { delta_time_ns_i as u64 } else { 0 };
                let delta_cpu_time = if cpu_time >= data.last_cpu_time {
                    cpu_time - data.last_cpu_time
                } else {
                    0
                };
                if delta_time_ns > 0 {
                    thread.cpu_usage_percent = (delta_cpu_time as f64 / delta_time_ns as f64) * 100.0;
                    if thread.cpu_usage_percent > 100.0 {
                        thread.cpu_usage_percent = 100.0;
                    }
                }
                break;
            }
        }
    }

    let mut name = String::new();
    if let Ok(mut f) = File::open(&comm_path) {
        let mut buf = [0u8; MAX_THREAD_LEN - 1];
        if let Ok(n) = f.read(&mut buf) {
            if n > 0 {
                name = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            }
        }
    }
    if name.is_empty() && !ctx.permissions_adjusted {
        crate::procscan::setup_permissions(ctx);
        if let Ok(mut f) = File::open(&comm_path) {
            let mut buf = [0u8; MAX_THREAD_LEN - 1];
            if let Ok(n) = f.read(&mut buf) {
                if n > 0 {
                    name = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                }
            }
        }
    }

    if name.is_empty() {
        let status_path = format!("/proc/{}/task/{}/status", pid, tid);
        if let Ok(mut f) = File::open(&status_path) {
            let mut buf = [0u8; 1024];
            if let Ok(n) = f.read(&mut buf) {
                let s = String::from_utf8_lossy(&buf[..n]);
                if let Some(name_start) = s.find("Name:") {
                    let rest = &s[name_start + 5..];
                    if let Some(name_end) = rest.find('\n') {
                        name = rest[..name_end].trim().to_string();
                    }
                }
            }
        }
    }
    if name.is_empty() {
        name = format!("unknown_{}", tid);
    }
    thread.name = name;

    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_ZERO(&mut set) };
    let rc = unsafe { sched_getaffinity(tid, std::mem::size_of::<cpu_set_t>(), &mut set) };
    if rc < 0 {
        return None;
    }
    thread.affinity = set;

    Some(thread)
}

pub fn get_threads_by_pid(
    ctx: &mut SystemContext,
    pid: i32,
    pkg_idx: usize,
) -> Option<PackageThreads> {
    let tids = scan_thread_ids(pid)?;
    let curr_time = mono_time();
    let mut pkg_threads = PackageThreads {
        threads: Vec::new(),
        num_threads: 0,
    };
    // 确保外层 Vec 有 pkg_idx 对应的内层槽区
    while ctx.last_thread_data.len() <= pkg_idx {
        ctx.last_thread_data.push(Vec::new());
    }
    // 记录本轮命中的历史槽位索引，用于扫描结束后回收已退出线程的槽位
    let mut used_slots: Vec<usize> = Vec::with_capacity(tids.len());
    for tid in tids {
        if let Some(thread) = read_thread_info(ctx, pid, tid, pkg_idx, curr_time) {
            if ctx.affinity_initialized {
                let slots = &mut ctx.last_thread_data[pkg_idx];
                let mut found_idx: Option<usize> = None;
                for (i, data) in slots.iter().enumerate() {
                    if data.pid == pid && data.tid == tid {
                        found_idx = Some(i);
                        break;
                    }
                }
                if let Some(i) = found_idx {
                    slots[i].last_cpu_time = thread.cpu_time;
                    slots[i].last_time = curr_time;
                    used_slots.push(i);
                } else {
                    // 找空槽复用（线程退出后被回收留下的 0 槽），否则 push 新槽
                    let mut empty_idx: Option<usize> = None;
                    for (i, data) in slots.iter().enumerate() {
                        if data.pid == 0 && data.tid == 0 {
                            empty_idx = Some(i);
                            break;
                        }
                    }
                    match empty_idx {
                        Some(i) => {
                            slots[i] = ThreadData {
                                pid,
                                tid,
                                last_cpu_time: thread.cpu_time,
                                last_time: curr_time,
                            };
                            used_slots.push(i);
                        }
                        None => {
                            slots.push(ThreadData {
                                pid,
                                tid,
                                last_cpu_time: thread.cpu_time,
                                last_time: curr_time,
                            });
                            used_slots.push(slots.len() - 1);
                        }
                    }
                }
            }
            pkg_threads.threads.push(thread);
            pkg_threads.num_threads += 1;
        }
    }

    // 回收本包槽区中本轮未命中的槽位（线程已退出），槽位复用避免 Vec 无限增长
    if ctx.affinity_initialized {
        let slots = &mut ctx.last_thread_data[pkg_idx];
        for (i, data) in slots.iter_mut().enumerate() {
            if !used_slots.contains(&i) {
                data.pid = 0;
                data.tid = 0;
                data.last_cpu_time = 0;
            }
        }
    }

    if pkg_threads.num_threads == 0 {
        return None;
    }

    ctx.affinity_initialized = true;
    Some(pkg_threads)
}