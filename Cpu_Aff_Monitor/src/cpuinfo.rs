use std::fs;
use crate::types::{CpuCluster, CpuInfo, SystemContext};
use crate::utils::get_nprocs;

fn read_i64(path: &str) -> i64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// 给已排序（容量降序）的簇分配 AppOptR 语义名，兼容 1..=N 档拓扑。
/// 返回 (has_m, has_p)。
///
/// - 1 簇：all-core
/// - 2 簇：e-core + hp-core
/// - 3 簇：e-core + p-core + hp-core
/// - 4+ 簇：e-core + m-core + p-core×（中间档）+ hp-core
fn assign_semantics(clusters: &mut [CpuCluster]) -> (bool, bool) {
    let n = clusters.len();
    if n == 1 {
        clusters[0].semantic = "all-core".to_string();
        clusters[0].type_name = "大核".to_string();
        return (false, false);
    }
    // clusters[0] 是最高档。
    clusters[0].semantic = "hp-core".to_string();
    clusters[0].type_name = "超大核".to_string();
    // 最低档固定为 e-core。
    let last = n - 1;
    clusters[last].semantic = "e-core".to_string();
    clusters[last].type_name = "小核".to_string();
    if n == 2 {
        return (false, false);
    }
    if n == 3 {
        clusters[1].semantic = "p-core".to_string();
        clusters[1].type_name = "大核".to_string();
        return (false, true);
    }
    // n >= 4：中间高段进 p-core，次低进 m-core，多出来的中间档折进 p-core。
    for i in 1..(n - 2) {
        clusters[i].semantic = "p-core".to_string();
        clusters[i].type_name = "大核".to_string();
    }
    clusters[n - 2].semantic = "m-core".to_string();
    clusters[n - 2].type_name = "中核".to_string();
    (true, true)
}

/// 无 /sys 可读时按核心数启发式合成簇（兜底，仅保证不崩溃）。
fn synthesize_fallback(ctx: &mut SystemContext) {
    let total = get_nprocs();
    ctx.num_cpus = total;
    ctx.cpu_ids = (0..total as i32).collect();
    let nclusters = if total >= 8 {
        3
    } else if total >= 5 {
        2
    } else {
        1
    };
    ctx.num_clusters = nclusters;
    let mut clusters: Vec<CpuCluster> = Vec::new();
    match nclusters {
        1 => {
            clusters.push(CpuCluster {
                start_cpu: 0,
                end_cpu: (total - 1) as i32,
                capacity: 1024,
                max_freq: 3000000,
                type_name: String::new(),
                semantic: String::new(),
            });
        }
        2 => {
            let big_start = (total - 1) as i32;
            clusters.push(CpuCluster {
                start_cpu: big_start,
                end_cpu: big_start,
                capacity: 1024,
                max_freq: 2800000,
                type_name: String::new(),
                semantic: String::new(),
            });
            clusters.push(CpuCluster {
                start_cpu: 0,
                end_cpu: (total - 2) as i32,
                capacity: 512,
                max_freq: 1800000,
                type_name: String::new(),
                semantic: String::new(),
            });
        }
        _ => {
            let super_id = (total - 1) as i32;
            let big_id = (total - 2) as i32;
            clusters.push(CpuCluster {
                start_cpu: super_id,
                end_cpu: super_id,
                capacity: 1024,
                max_freq: 3000000,
                type_name: String::new(),
                semantic: String::new(),
            });
            clusters.push(CpuCluster {
                start_cpu: big_id,
                end_cpu: big_id,
                capacity: 768,
                max_freq: 2500000,
                type_name: String::new(),
                semantic: String::new(),
            });
            clusters.push(CpuCluster {
                start_cpu: 0,
                end_cpu: (total - 3) as i32,
                capacity: 512,
                max_freq: 1800000,
                type_name: String::new(),
                semantic: String::new(),
            });
        }
    }
    assign_semantics(&mut clusters);
    ctx.clusters = clusters;
}

pub fn init_cpu_info(ctx: &mut SystemContext) {
    ctx.cpu_info.clear();
    ctx.clusters.clear();
    ctx.cpu_ids.clear();
    ctx.num_cpus = 0;
    ctx.num_clusters = 0;

    let mut freq_available = false;
    let mut capacity_available = false;
    let mut cpu_info: Vec<CpuInfo> = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("cpu") || name.len() <= 3 {
                continue;
            }
            let suffix = &name[3..];
            if !suffix.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let cpu_id: i32 = match suffix.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let max_freq = read_i64(&format!(
                "/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_max_freq",
                cpu_id
            ));
            if max_freq > 0 {
                freq_available = true;
            }
            let capacity = read_i64(&format!(
                "/sys/bus/cpu/devices/cpu{}/cpu_capacity",
                cpu_id
            ));
            if capacity > 0 {
                capacity_available = true;
            }
            cpu_info.push(CpuInfo {
                cpu_id,
                max_freq,
                capacity,
            });
        }
    }

    if cpu_info.is_empty() {
        synthesize_fallback(ctx);
        return;
    }

    // 容量优先，其次频率；按能力降序，cpu_id 升序保证确定性。
    cpu_info.sort_by(|a, b| {
        b.capacity
            .cmp(&a.capacity)
            .then(b.max_freq.cmp(&a.max_freq))
            .then(a.cpu_id.cmp(&b.cpu_id))
    });

    let use_freq = freq_available && !capacity_available;
    let mut clusters: Vec<CpuCluster> = Vec::new();
    let mut last_value: i64 = -1;
    let mut cluster_start = 0usize;
    for i in 0..cpu_info.len() {
        let cur = if use_freq {
            cpu_info[i].max_freq
        } else {
            cpu_info[i].capacity
        };
        if cur != last_value && last_value != -1 {
            // 簇内 cpu_id 可能因容量降序而乱序，取最小/最大作为范围（升序展示）。
            let group = &cpu_info[cluster_start..i];
            let lo = group.iter().map(|c| c.cpu_id).min().unwrap_or(0);
            let hi = group.iter().map(|c| c.cpu_id).max().unwrap_or(0);
            clusters.push(CpuCluster {
                start_cpu: lo,
                end_cpu: hi,
                capacity: last_value,
                max_freq: cpu_info[cluster_start].max_freq,
                type_name: String::new(),
                semantic: String::new(),
            });
            cluster_start = i;
        }
        last_value = cur;
    }
    if cluster_start < cpu_info.len() {
        let group = &cpu_info[cluster_start..];
        let lo = group.iter().map(|c| c.cpu_id).min().unwrap_or(0);
        let hi = group.iter().map(|c| c.cpu_id).max().unwrap_or(0);
        clusters.push(CpuCluster {
            start_cpu: lo,
            end_cpu: hi,
            capacity: last_value,
            max_freq: cpu_info[cluster_start].max_freq,
            type_name: String::new(),
            semantic: String::new(),
        });
    }

    let (has_m, has_p) = assign_semantics(&mut clusters);
    ctx.has_m = has_m;
    ctx.has_p = has_p;
    ctx.cpu_info = cpu_info.clone();
    ctx.clusters = clusters;
    ctx.num_cpus = cpu_info.len();
    ctx.num_clusters = ctx.clusters.len();
    ctx.cpu_ids = cpu_info.iter().map(|c| c.cpu_id).collect();
    // 亲和显示需按升序遍历（affinity_to_string 依赖升序建区间）。
    ctx.cpu_ids.sort();
}
