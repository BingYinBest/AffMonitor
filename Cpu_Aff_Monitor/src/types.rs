use libc::{cpu_set_t, pid_t};
use std::collections::HashMap;
use std::rc::Rc;

/// 单线程采样快照。cpu_usage_percent 为 fast EWMA（当前负载），cpu_usage_slow 为 slow EWMA（基线）。
#[derive(Clone)]
pub struct ThreadInfo {
    pub name: String,
    pub affinity: cpu_set_t,
    pub cpu_usage_percent: f64,
    pub cpu_usage_slow: f64,
    /// 是否为进程主线程（tid == tgid）。
    pub main: bool,
}

#[derive(Clone, Copy)]
pub struct CpuInfo {
    pub cpu_id: i32,
    pub max_freq: i64,
    pub capacity: i64,
}

/// 一个 CPU 簇（同性能/频率分组）。
#[derive(Clone)]
pub struct CpuCluster {
    pub start_cpu: i32,
    pub end_cpu: i32,
    pub capacity: i64,
    pub max_freq: i64,
    /// 展示用类型名：小核 / 中核 / 大核 / 超大核。
    pub type_name: String,
    /// AppOptR 语义名：e-core / m-core / p-core / hp-core / all-core。
    pub semantic: String,
}

/// 一条生成/历史规则。thread_pattern 为 AppOptR 线程名模式（可能含 fnmatch 通配）。
#[derive(Clone)]
pub struct RuleInfo {
    pub pkg: String,
    pub thread_pattern: String,
    /// AppOptR 语义名，如 "hp-core" / "p-core,hp-core"。
    pub semantic: String,
    pub cpu_usage_percent: f64,
}

/// 某线程跨采样的持久状态（按 (pkg, tid) 索引）。
#[derive(Clone)]
pub struct ThreadState {
    pub pid: pid_t,
    pub last_cpu_time: u64,
    pub last_time: libc::timespec,
    /// fast EWMA（当前负载 %）。
    pub fast: f64,
    /// slow EWMA（基线 %）。
    pub slow: f64,
    /// 缓存的 comm 名（避免每轮重读）。
    pub name: String,
    /// 上次读取 comm 的轮次。
    pub name_read_round: u64,
    /// 是否有上一基线（可计算 delta）。
    pub initialized: bool,
    /// EWMA 是否已播种（首样本直接取瞬时值）。
    pub seeded: bool,
    /// 本轮是否仍存活（每轮开始清零，扫描结束后据此回收）。
    pub seen: bool,
}

impl ThreadState {
    pub fn new(pid: pid_t) -> Self {
        ThreadState {
            pid,
            last_cpu_time: 0,
            last_time: libc::timespec { tv_sec: 0, tv_nsec: 0 },
            fast: 0.0,
            slow: 0.0,
            name: String::new(),
            name_read_round: 0,
            initialized: false,
            seeded: false,
            seen: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PidCache {
    pub pid: pid_t,
    pub last_update: i64,
}

#[derive(Clone)]
pub struct PackageThreads {
    pub threads: Vec<ThreadInfo>,
    pub num_threads: usize,
}

impl Default for PackageThreads {
    fn default() -> Self {
        PackageThreads {
            threads: Vec::new(),
            num_threads: 0,
        }
    }
}

pub struct SystemContext {
    pub cpu_info: Vec<CpuInfo>,
    pub clusters: Vec<CpuCluster>,
    pub num_cpus: usize,
    pub num_clusters: usize,
    /// 实际存在的 CPU id（升序）。亲和显示按此遍历，兼容非连续/离线 CPU。
    pub cpu_ids: Vec<i32>,
    /// 是否存在中核档（4+ 簇）。
    pub has_m: bool,
    /// 是否存在大核档（3+ 簇）。
    pub has_p: bool,
    /// 包名 -> (pid, last_update 秒)。修复按 pkg_idx 索引导致的错位。
    pub pid_cache: HashMap<String, PidCache>,
    /// 历史规则（按 (pkg, thread_pattern) 去重，键不含 CPU 范围）。
    pub historical_rules: Vec<RuleInfo>,
    pub historical_rules_capacity: usize,
    /// 包名 -> (tid -> 线程状态)。修复 pkg_idx 错位。
    pub thread_states: HashMap<String, HashMap<i32, ThreadState>>,
    /// 包名 -> 最后前台快照（后台包复现用，Rc 零拷贝）。
    pub last_foreground: HashMap<String, Rc<PackageThreads>>,
    pub affinity_initialized: bool,
    pub cached_top_pkg: String,
    pub last_top_pkg_time: i64,
    pub permissions_adjusted: bool,
    pub original_selinux_state: i32,
    /// 采样轮次计数。
    pub round: u64,
}
