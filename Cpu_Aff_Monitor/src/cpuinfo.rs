use std::fs;
use crate::types::{CpuCluster, CpuInfo};
use crate::utils::get_nprocs;

fn read_i64(path: &str) -> i64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

pub fn init_cpu_info(
    cpu_info: &mut Vec<CpuInfo>,
    clusters: &mut Vec<CpuCluster>,
    num_cpus: &mut usize,
    num_clusters: &mut usize,
    little_cluster_end: &mut i32,
    big_cluster_start: &mut i32,
    super_core: &mut i32,
) {
    let total = get_nprocs();
    cpu_info.clear();
    clusters.clear();
    *num_cpus = 0;
    *num_clusters = 0;
    *little_cluster_end = -1;
    *big_cluster_start = -1;
    *super_core = -1;

    let mut freq_available = false;
    let mut capacity_available = false;

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
            if cpu_info.len() >= total {
                break;
            }
            let cpu_id: i32 = suffix.parse().unwrap_or(-1);
            if cpu_id < 0 {
                continue;
            }
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
        *num_cpus = total;
        if *num_cpus <= 0 {
            eprintln!("错误: 无法获取 CPU 核心数，程序退出");
            std::process::exit(1);
        }
        *num_clusters = if *num_cpus >= 3 { 3 } else { *num_cpus };
        for i in 0..*num_clusters {
            let mut c = CpuCluster {
                start_cpu: 0,
                end_cpu: 0,
                capacity: 0,
                max_freq: 0,
                type_name: String::new(),
            };
            if i == 0 && *num_clusters >= 1 {
                c.start_cpu = (*num_cpus - 1) as i32;
                c.end_cpu = (*num_cpus - 1) as i32;
                c.capacity = 1024;
                c.max_freq = 3000000;
                c.type_name = "超大核".to_string();
                *super_core = (*num_cpus - 1) as i32;
            } else if i == 1 && *num_clusters >= 2 {
                c.start_cpu = if *num_cpus >= 2 { (*num_cpus - 2) as i32 } else { 0 };
                c.end_cpu = if *num_cpus >= 2 { (*num_cpus - 2) as i32 } else { 0 };
                c.capacity = 768;
                c.max_freq = 2500000;
                c.type_name = "大核".to_string();
                *big_cluster_start = c.start_cpu;
            } else if i == 2 && *num_clusters >= 3 {
                c.start_cpu = 0;
                c.end_cpu = if *num_cpus >= 3 { (*num_cpus - 3) as i32 } else { 0 };
                c.capacity = 512;
                c.max_freq = 1800000;
                c.type_name = "小核".to_string();
                *little_cluster_end = c.end_cpu;
            }
            clusters.push(c);
        }
        println!("CPU 核心簇信息（默认配置）:");
        for (i, c) in clusters.iter().enumerate() {
            if c.start_cpu == c.end_cpu {
                println!("簇 {}: CPU {}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.capacity, c.max_freq, c.type_name);
            } else {
                println!("簇 {}: CPU {}-{}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.end_cpu, c.capacity, c.max_freq, c.type_name);
            }
        }
        return;
    }

    cpu_info.sort_by(|a, b| {
        b.capacity
            .cmp(&a.capacity)
            .then(b.max_freq.cmp(&a.max_freq))
            .then(a.cpu_id.cmp(&b.cpu_id))
    });

    let use_freq = freq_available && !capacity_available;
    let mut last_value: i64 = -1;
    let mut cluster_start = 0usize;
    let mut cluster_count = 0usize;
    for i in 0..cpu_info.len() {
        let current_value = if use_freq {
            cpu_info[i].max_freq
        } else {
            cpu_info[i].capacity
        };
        if current_value != last_value && last_value != -1 {
            clusters.push(CpuCluster {
                start_cpu: cpu_info[cluster_start].cpu_id,
                end_cpu: cpu_info[i - 1].cpu_id,
                capacity: last_value,
                max_freq: cpu_info[cluster_start].max_freq,
                type_name: String::new(),
            });
            cluster_count += 1;
            cluster_start = i;
        }
        last_value = current_value;
    }
    if cluster_start < cpu_info.len() {
        clusters.push(CpuCluster {
            start_cpu: cpu_info[cluster_start].cpu_id,
            end_cpu: cpu_info[cpu_info.len() - 1].cpu_id,
            capacity: last_value,
            max_freq: cpu_info[cluster_start].max_freq,
            type_name: String::new(),
        });
        cluster_count += 1;
    }
    *num_clusters = cluster_count;

    for i in 0..*num_clusters {
        if *num_clusters == 1 {
            clusters[i].type_name = "大核".to_string();
            *big_cluster_start = clusters[i].start_cpu;
        } else if *num_clusters == 2 {
            clusters[i].type_name = if i == 0 { "大核".to_string() } else { "小核".to_string() };
            if i == 0 {
                *big_cluster_start = clusters[i].start_cpu;
            } else {
                *little_cluster_end = clusters[i].end_cpu;
            }
        } else {
            if i == 0 {
                clusters[i].type_name = "超大核".to_string();
                *super_core = clusters[i].start_cpu;
            } else if i == *num_clusters - 1 {
                clusters[i].type_name = "小核".to_string();
                *little_cluster_end = clusters[i].end_cpu;
            } else {
                clusters[i].type_name = "大核".to_string();
                if *big_cluster_start == -1 {
                    *big_cluster_start = clusters[i].start_cpu;
                }
            }
        }
    }

    *num_cpus = cpu_info.len();

    println!("CPU 核心簇信息:");
    for (i, c) in clusters.iter().enumerate() {
        if c.start_cpu == c.end_cpu {
            println!("簇 {}: CPU {}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.capacity, c.max_freq, c.type_name);
        } else {
            println!("簇 {}: CPU {}-{}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.end_cpu, c.capacity, c.max_freq, c.type_name);
        }
    }
}