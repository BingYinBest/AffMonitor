use std::fs::{self, File};
use std::io::Read;
use libc::{cpu_set_t, CPU_ZERO, sched_getaffinity};
use crate::constants::*;
use crate::types::{PackageThreads, SystemContext, ThreadInfo, ThreadState};
use crate::utils::mono_time;
use std::cmp::Ordering;

/// 扫描 /proc/<pid>/task，返回线程 tid 列表。
pub fn scan_thread_ids(pid: i32) -> Option<Vec<i32>> {
    let task_path = format!("/proc/{}/task", pid);
    let entries = fs::read_dir(&task_path).ok()?;
    let mut tids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(tid) = name_str.parse::<i32>() {
                tids.push(tid);
            }
        }
    }
    Some(tids)
}

/// 读一个 u64 文本文件（栈缓冲，零堆分配）。读取失败返回 None。
fn read_u64(path: &str) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut buf = [0u8; 32];
    let n = f.read(&mut buf).ok()?;
    let s = std::str::from_utf8(&buf[..n]).ok()?;
    s.trim().parse::<u64>().ok()
}

/// 读 comm（栈缓冲，按 COMM_LEN 定长），返回字节数。
fn read_comm(path: &str, buf: &mut [u8; COMM_LEN + 2]) -> usize {
    if let Ok(mut f) = File::open(path) {
        if let Ok(n) = f.read(buf) {
            return n;
        }
    }
    0
}

/// 从 stat 的 utime+stime 回退解析 CPU 时间（仅当内核无 schedstat 时）。
fn read_stat_cpu_time(pid: i32, tid: i32) -> Option<u64> {
    let path = format!("/proc/{}/task/{}/stat", pid, tid);
    let mut f = File::open(&path).ok()?;
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).ok()?;
    let s = String::from_utf8_lossy(&buf[..n]);
    let rparen = s.rfind(')')?;
    let after = &s[rparen + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    if fields.len() > 12 {
        if let (Ok(ut), Ok(st)) = (fields[11].parse::<u64>(), fields[12].parse::<u64>()) {
            let clk = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            if clk > 0 {
                return Some((ut + st) * 1_000_000_000u64 / (clk as u64));
            }
        }
    }
    None
}

/// 读取一个线程的快照并更新其跨采样状态（EWMA / 名字缓存）。
/// `states` 是本包线程状态表（按 tid 索引）。
fn read_thread_info(
    pid: i32,
    tid: i32,
    states: &mut std::collections::HashMap<i32, ThreadState>,
    curr_time: libc::timespec,
    round: u64,
) -> Option<ThreadInfo> {
    let sched_path = format!("/proc/{}/task/{}/schedstat", pid, tid);
    let comm_path = format!("/proc/{}/task/{}/comm", pid, tid);

    // schedstat 优先；读取失败（内核无 schedstat）才回退 stat。
    let cpu_time = match read_u64(&sched_path) {
        Some(v) => v,
        None => read_stat_cpu_time(pid, tid)?,
    };

    let state = states
        .entry(tid)
        .or_insert_with(|| ThreadState::new(pid));

    // 计算瞬时占用率并更新双 EWMA。
    let mut fast = 0.0;
    let mut slow = 0.0;
    if state.initialized && state.pid == pid {
        let delta_sec = (curr_time.tv_sec - state.last_time.tv_sec) as i64;
        let delta_nsec = (curr_time.tv_nsec - state.last_time.tv_nsec) as i64;
        let delta_ns = delta_sec * 1_000_000_000i64 + delta_nsec;
        let delta_time = if delta_ns > 0 { delta_ns as u64 } else { 0 };
        let delta_cpu = if cpu_time >= state.last_cpu_time {
            cpu_time - state.last_cpu_time
        } else {
            0
        };
        let inst = if delta_time > 0 {
            ((delta_cpu as f64 / delta_time as f64) * 100.0).min(100.0)
        } else {
            0.0
        };
        if !state.seeded {
            state.fast = inst;
            state.slow = inst;
            state.seeded = true;
        } else {
            state.fast = (EWMA_FAST_W * inst + (1.0 - EWMA_FAST_W) * state.fast).min(100.0);
            state.slow = (EWMA_SLOW_W * inst + (1.0 - EWMA_SLOW_W) * state.slow).min(100.0);
        }
        fast = state.fast;
        slow = state.slow;
    } else {
        // 新线程（或 tid 被新进程复用）：重建基线，本轮占用率记 0。
        state.pid = pid;
        state.initialized = true;
        state.seeded = false;
        state.fast = 0.0;
        state.slow = 0.0;
    }
    state.last_cpu_time = cpu_time;
    state.last_time = curr_time;
    state.seen = true;

    // 名字缓存：仅在首次或每 COMM_REREAD_INTERVAL 轮重读 comm，避免每轮打开文件。
    let name = if state.name.is_empty()
        || round.wrapping_sub(state.name_read_round) >= COMM_REREAD_INTERVAL
    {
        let mut buf = [0u8; COMM_LEN + 2];
        let n = read_comm(&comm_path, &mut buf);
        if n > 0 {
            let s = std::str::from_utf8(&buf[..n]).unwrap_or("");
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                state.name = trimmed.to_string();
            }
        }
        state.name_read_round = round;
        state.name.clone()
    } else {
        state.name.clone()
    };
    let name = if name.is_empty() {
        format!("unknown_{}", tid)
    } else {
        name
    };

    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_ZERO(&mut set) };
    let rc = unsafe { sched_getaffinity(tid, std::mem::size_of::<cpu_set_t>(), &mut set) };
    if rc < 0 {
        // 线程已退出或不可访问，标记为未存活以便回收。
        state.seen = false;
        return None;
    }

    Some(ThreadInfo {
        name,
        affinity: set,
        cpu_usage_percent: fast,
        cpu_usage_slow: slow,
        main: tid == pid,
    })
}

/// 扫描一个包进程的所有线程，返回快照。
pub fn get_threads_by_pid(ctx: &mut SystemContext, pkg: &str, pid: i32) -> Option<PackageThreads> {
    let tids = scan_thread_ids(pid)?;
    let curr_time = mono_time();

    // 权限初始化：若首个线程 comm 不可读，则触发 chmod / SELinux 降级。
    if !ctx.permissions_adjusted {
        if let Some(&first) = tids.first() {
            let comm_path = format!("/proc/{}/task/{}/comm", pid, first);
            if !crate::procscan::can_read_path(&comm_path) {
                crate::procscan::setup_permissions(ctx, &comm_path);
            }
        }
    }

    // 本包线程状态表（按 tid 索引），修复按 pkg_idx 的错位。
    let states = ctx.thread_states.entry(pkg.to_string()).or_default();
    for s in states.values_mut() {
        s.seen = false;
    }

    let mut pkg_threads = PackageThreads::default();
    for tid in tids {
        if let Some(thread) = read_thread_info(pid, tid, states, curr_time, ctx.round) {
            pkg_threads.threads.push(thread);
            pkg_threads.num_threads += 1;
        }
    }

    // 回收本轮未命中的线程状态（已退出）。
    states.retain(|_, s| s.seen);
    // 防内存无限增长：超限时淘汰占用率最低的线程状态。
    if states.len() > MAX_THREADS_PER_PKG {
        let mut keys: Vec<(i32, f64)> = states.iter().map(|(k, v)| (*k, v.fast)).collect();
        keys.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        let excess = states.len() - MAX_THREADS_PER_PKG;
        for (k, _) in keys.into_iter().take(excess) {
            states.remove(&k);
        }
    }

    if pkg_threads.num_threads == 0 {
        return None;
    }

    ctx.affinity_initialized = true;
    Some(pkg_threads)
}
