use libc::{cpu_set_t, pid_t};

#[derive(Clone)]
pub struct ThreadInfo {
    pub name: String,
    pub cpu_time: u64,
    pub affinity: cpu_set_t,
    pub cpu_usage_percent: f64,
    pub tid: pid_t,
}

impl ThreadInfo {
    pub fn new() -> Self {
        ThreadInfo {
            name: String::new(),
            cpu_time: 0,
            affinity: unsafe { std::mem::zeroed() },
            cpu_usage_percent: 0.0,
            tid: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct CpuInfo {
    pub cpu_id: i32,
    pub max_freq: i64,
    pub capacity: i64,
}

#[derive(Clone)]
pub struct CpuCluster {
    pub start_cpu: i32,
    pub end_cpu: i32,
    pub capacity: i64,
    pub max_freq: i64,
    pub type_name: String,
}

#[derive(Clone)]
pub struct RuleInfo {
    pub pkg: String,
    pub thread_name: String,
    pub start_cpu: i32,
    pub end_cpu: i32,
    pub cpu_usage_percent: f64,
}

#[derive(Clone, Copy)]
pub struct ThreadData {
    pub pid: pid_t,
    pub tid: pid_t,
    pub last_cpu_time: u64,
    pub last_time: libc::timespec,
}

#[derive(Clone)]
pub struct PidCache {
    pub pkg: String,
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
    pub little_cluster_end: i32,
    pub big_cluster_start: i32,
    pub super_core: i32,
    /// 外层按包索引，内层为该包的线程历史槽位（按需增长，避免预分配 128*1024）
    pub last_thread_data: Vec<Vec<ThreadData>>,
    pub pid_cache: Vec<PidCache>,
    pub historical_rules: Vec<RuleInfo>,
    pub historical_rules_capacity: usize,
    pub last_foreground: Vec<PackageThreads>,
    pub affinity_initialized: bool,
    pub cached_top_pkg: String,
    pub last_top_pkg_time: i64,
    pub permissions_adjusted: bool,
    pub original_selinux_state: i32,
}