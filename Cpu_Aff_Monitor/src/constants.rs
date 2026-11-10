// 全局常量。所有调参集中在这里，改阈值只需改这一处。

/// 最大包名长度（用于缓冲/截断）。
pub const MAX_PKG_LEN: usize = 128;
/// 内核 TASK_COMM_LEN-1，comm 实际最长 15 字节。
pub const COMM_LEN: usize = 15;
/// exec_cmd 的读取缓冲大小。
pub const CMD_BUFFER_SIZE: usize = 65536;
/// 目标包数量上限。
pub const MAX_PACKAGES: usize = 128;
/// 活跃包采样间隔（秒）。
pub const UPDATE_INTERVAL: u64 = 5;
/// 全部后台时的省电采样间隔（秒）。
pub const IDLE_INTERVAL: u64 = 15;
/// 判定线程"值得关注"的最低占用率（%）。
pub const HIGH_USAGE_THRESHOLD: f64 = 5.0;
/// exec_cmd 超时（秒）。
pub const CMD_TIMEOUT_SECONDS: libc::c_uint = 5;
/// 历史规则数量上限。
pub const MAX_HISTORICAL_RULES: usize = 2048;
/// 单包线程状态上限（防内存无限增长）。
pub const MAX_THREADS_PER_PKG: usize = 4096;
/// 单个包生成规则条数上限（防规则表爆炸）。
pub const RULE_MAX_PER_PKG: usize = 48;
/// comm 名缓存的刷新周期（轮）。线程名极少改名，周期重读兜底。
pub const COMM_REREAD_INTERVAL: u64 = 20;
/// 前台应用缓存时长（秒）。
pub const TOP_PKG_CACHE_SECS: i64 = 3;

/// EWMA fast 权重（跟随当前负载）。
pub const EWMA_FAST_W: f64 = 0.6;
/// EWMA slow 权重（代表基线）。
pub const EWMA_SLOW_W: f64 = 0.15;

/// 线程占用率分级阈值（%）。
pub const TIER_SUPER_THRESHOLD: f64 = 40.0;
pub const TIER_BIG_THRESHOLD: f64 = 20.0;
pub const TIER_MID_THRESHOLD: f64 = 8.0;

/// 主线程判定：tid == tgid（进程主线程）。
pub const TIER_LITTLE: u8 = 0;
pub const TIER_MID: u8 = 1;
pub const TIER_BIG: u8 = 2;
pub const TIER_SUPER: u8 = 3;
