use crate::constants::*;
use crate::types::{CpuCluster, PackageThreads, RuleInfo, SystemContext, ThreadInfo};
use crate::utils::appoptr_pattern;
use std::collections::HashMap;
use std::fmt::Write;

/// 给定簇列表和目标 rank（0=little..3=super），返回该 rank 对应的显式 CPU 范围字符串。
/// 簇必须按 capacity 降序排列（由 init_cpu_info 保证）。
fn rank_to_cpu_range(rank: u8, clusters: &[CpuCluster]) -> String {
    let n = clusters.len();
    match n {
        1 => {
            let c = &clusters[0];
            if c.start_cpu == c.end_cpu {
                c.start_cpu.to_string()
            } else {
                format!("{}-{}", c.start_cpu, c.end_cpu)
            }
        }
        2 => {
            if rank == 0 {
                let c = &clusters[1];
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            } else {
                let c = &clusters[0];
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            }
        }
        3 => match rank {
            0 => {
                let c = &clusters[2];
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            }
            1 => {
                let c = &clusters[1];
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            }
            _ => {
                let c = &clusters[0];
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            }
        },
        _ => {
            let idx = if rank as usize >= n { n - 1 } else { rank as usize };
            let c = &clusters[idx];
            if c.start_cpu == c.end_cpu {
                c.start_cpu.to_string()
            } else {
                format!("{}-{}", c.start_cpu, c.end_cpu)
            }
        }
    }
}

/// 线程类别检测：返回 (AppOptR 线程模式, 是否后台线程)。
/// 后台维护线程（JIT/GC/Finalizer 等）即使忙也压小核；关键渲染/UI 线程给大核。
fn category(name: &str) -> Option<(&'static str, bool)> {
    let n = name.to_lowercase();
    // ---- 后台维护线程：始终 e-core ----
    if n.contains("jit") {
        return Some(("Jit*", true));
    }
    if n.contains("heap") {
        return Some(("HeapTask*", true));
    }
    if n.contains("finalizer") {
        return Some(("Finalizer*", true));
    }
    if n.contains("reference") {
        return Some(("ReferenceQueue*", true));
    }
    if n.contains("profile") {
        return Some(("Profile*", true));
    }
    if n.contains("cleanup") {
        return Some(("Cleanup*", true));
    }
    if n.contains("gc") {
        return Some(("Gc*", true));
    }
    if n.contains("hwui") && n.contains("upload") {
        return Some(("hwui*Upload*", true));
    }
    // ---- 关键渲染/UI/交互线程 ----
    if n.contains("renderthread") {
        return Some(("RenderThread*", false));
    }
    if n.starts_with("hwui") {
        return Some(("hwui*", false));
    }
    if n.contains("glthread") {
        return Some(("GLThread*", false));
    }
    if n.contains("egl") {
        return Some(("Egl*", false));
    }
    if n.contains("gles") {
        return Some(("libGLES*", false));
    }
    if n.contains("render") {
        return Some(("Render*", false));
    }
    if n == "main" || n.starts_with("main") {
        return Some(("main*", false));
    }
    if n.starts_with("binder") {
        return Some(("binder*", false));
    }
    None
}

/// 计算线程期望档位（0=小核..3=超大核），返回 (规则模式, 档位)。
/// 返回 None 表示不值得生成独立规则（落回包级默认）。
fn desired_rank(t: &ThreadInfo) -> Option<(String, u8)> {
    let usage = t.cpu_usage_percent;
    if usage < HIGH_USAGE_THRESHOLD {
        return None;
    }
    // 已知类别（后台/渲染/绑定）优先用类别模式。
    if let Some((pattern, is_bg)) = category(&t.name) {
        if is_bg {
            return Some((pattern.to_string(), TIER_LITTLE));
        }
        let rank = if usage >= TIER_BIG_THRESHOLD {
            TIER_SUPER
        } else {
            TIER_BIG
        };
        return Some((pattern.to_string(), rank));
    }
    // 主线程（tid == tgid，Android 上 comm 是截断的包名）：用实际 comm 作模式。
    if t.main {
        if let Some(p) = appoptr_pattern(&t.name) {
            let rank = if usage >= TIER_BIG_THRESHOLD {
                TIER_SUPER
            } else {
                TIER_BIG
            };
            return Some((p.to_string(), rank));
        }
        return None;
    }
    // 通用线程：按占用率分级。
    let rank = if usage >= TIER_SUPER_THRESHOLD {
        TIER_SUPER
    } else if usage >= TIER_BIG_THRESHOLD {
        TIER_BIG
    } else if usage >= TIER_MID_THRESHOLD {
        TIER_MID
    } else {
        TIER_LITTLE
    };
    // 去抖：仅突发（fast 高、slow 基线低）不轻易占用超大核，降到 p-core。
    let rank = if rank == TIER_SUPER && t.cpu_usage_slow < 10.0 {
        TIER_BIG
    } else {
        rank
    };
    appoptr_pattern(&t.name).map(|p| (p.to_string(), rank))
}

fn update_historical(ctx: &mut SystemContext, pkg: &str, pattern: &str, semantic: &str, usage: f64) {
    for rule in ctx.historical_rules.iter_mut() {
        if rule.pkg == pkg && rule.thread_pattern == pattern {
            rule.semantic = semantic.to_string();
            rule.cpu_usage_percent = usage;
            return;
        }
    }
    if ctx.historical_rules.len() >= ctx.historical_rules_capacity {
        ctx.historical_rules.remove(0);
    }
    ctx.historical_rules.push(RuleInfo {
        pkg: pkg.to_string(),
        thread_pattern: pattern.to_string(),
        semantic: semantic.to_string(),
        cpu_usage_percent: usage,
    });
}

/// 包级默认亲和：系统进程偏大核，普通应用偏小核+大核；始终包含全部可用核心。
fn default_tier(is_system: bool, has_m: bool, has_p: bool, clusters: &[CpuCluster]) -> String {
    let all_cores: String = if clusters.is_empty() {
        "?".to_string()
    } else if clusters.len() == 1 {
        let c = &clusters[0];
        if c.start_cpu == c.end_cpu {
            c.start_cpu.to_string()
        } else {
            format!("{}-{}", c.start_cpu, c.end_cpu)
        }
    } else {
        // 全部簇范围用逗号连接
        clusters
            .iter()
            .map(|c| {
                if c.start_cpu == c.end_cpu {
                    c.start_cpu.to_string()
                } else {
                    format!("{}-{}", c.start_cpu, c.end_cpu)
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    if is_system {
        if has_p && clusters.len() >= 3 {
            // 系统：p-core + hp-core（中间档+最高档）
            let start_idx = if has_m { 1 } else { 2 };
            clusters[start_idx..]
                .iter()
                .map(|c| {
                    if c.start_cpu == c.end_cpu {
                        c.start_cpu.to_string()
                    } else {
                        format!("{}-{}", c.start_cpu, c.end_cpu)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        } else if clusters.len() >= 2 {
            // 只有2档：取最高档
            let c = &clusters[0];
            if c.start_cpu == c.end_cpu {
                c.start_cpu.to_string()
            } else {
                format!("{}-{}", c.start_cpu, c.end_cpu)
            }
        } else {
            all_cores
        }
    } else {
        if has_p && clusters.len() >= 3 {
            // 应用：e-core + p-core（最低档+中间档）
            let end_idx = if has_m { 2 } else { clusters.len() - 1 };
            clusters[..end_idx]
                .iter()
                .map(|c| {
                    if c.start_cpu == c.end_cpu {
                        c.start_cpu.to_string()
                    } else {
                        format!("{}-{}", c.start_cpu, c.end_cpu)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        } else if clusters.len() >= 2 {
            // 2档：e-core + hp-core
            if clusters.len() == 2 {
                format!(
                    "{},{}",
                    rank_to_cpu_range(0, clusters),
                    rank_to_cpu_range(1, clusters)
                )
            } else {
                all_cores
            }
        } else {
            all_cores
        }
    }
}

/// 生成 AppOptR 兼容规则，写入 out。
/// 规则按 (模式, 最高档) 分组，输出稳定、紧凑；
/// 当簇数 ≥ 2 时使用显式 CPU 范围（如 "4-5"），否则用语义名。
pub fn generate_rules(
    ctx: &mut SystemContext,
    pkg: &str,
    pkg_threads: &PackageThreads,
    out: &mut String,
    _is_foreground: bool,
    is_system: bool,
) {
    let clusters = &ctx.clusters;
    let threads = &pkg_threads.threads;

    // 1. 收集 (模式 -> (最高档, 最大占用))，同一模式合并取最高档。
    let mut groups: HashMap<String, (u8, f64)> = HashMap::new();
    for t in threads {
        if let Some((pattern, rank)) = desired_rank(t) {
            let entry = groups.entry(pattern).or_insert((rank, t.cpu_usage_percent));
            if rank > entry.0 {
                entry.0 = rank;
            }
            if t.cpu_usage_percent > entry.1 {
                entry.1 = t.cpu_usage_percent;
            }
        }
    }

    // 2. 按占用率降序，超限裁剪。
    let mut rules: Vec<(String, u8, f64)> = groups
        .into_iter()
        .map(|(p, (r, u))| (p, r, u))
        .collect();
    rules.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    if rules.len() > RULE_MAX_PER_PKG {
        rules.truncate(RULE_MAX_PER_PKG);
    }

    // 3. 包级默认亲和（覆盖未命中线程规则的其余线程）。
    let default = default_tier(is_system, ctx.has_m, ctx.has_p, clusters);

    // 4. 输出 AppOptR 兼容块（显式 CPU 范围格式）。
    if rules.is_empty() {
        let _ = writeln!(out, "{}={}", pkg, default);
    } else {
        let _ = writeln!(out, "{}={} {{", pkg, default);
        for (pattern, rank, _u) in &rules {
            let sem = rank_to_cpu_range(*rank, clusters);
            let _ = writeln!(out, "\t{}={}", pattern, sem);
        }
        let _ = writeln!(out, "}}");
    }

    // 5. 预计算所有语义字符串（避免同时借用 ctx 可变和不可变）。
    let mut active: Vec<&str> = Vec::with_capacity(rules.len());
    let mut hist_items: Vec<(&str, String, f64)> = Vec::with_capacity(rules.len());
    for (pattern, rank, usage) in &rules {
        let sem = rank_to_cpu_range(*rank, clusters);
        hist_items.push((pattern.as_str(), sem, *usage));
        active.push(pattern.as_str());
    }

    // 6. 更新历史规则（按 (pkg, pattern) 去重，键不含 CPU 范围）。
    for (pattern, sem, usage) in hist_items {
        update_historical(ctx, pkg, pattern, &sem, usage);
    }

    // 7. 历史注释：曾经忙但本轮不再活跃的线程，作为建议保留（不参与解析）。
    for rule in &ctx.historical_rules {
        if rule.pkg != pkg {
            continue;
        }
        if active.contains(&rule.thread_pattern.as_str()) {
            continue;
        }
        if rule.cpu_usage_percent >= 1.0 {
            let _ = writeln!(
                out,
                "# 历史: {}={} (历史占用 {:.2}%)",
                rule.thread_pattern, rule.semantic, rule.cpu_usage_percent
            );
        }
    }
}
