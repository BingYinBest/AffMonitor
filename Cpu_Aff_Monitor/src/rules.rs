use crate::constants::HIGH_USAGE_THRESHOLD;
use crate::types::{PackageThreads, RuleInfo, SystemContext};
use crate::utils::escape_thread_name;
use std::fmt::Write;

/// 线程分配层级：越高越该跑在大核/超大核
const TIER_LITTLE: u8 = 0;
const TIER_MID: u8 = 1;
const TIER_BIG: u8 = 2;
const TIER_SUPER: u8 = 3;

fn find_cluster_boundaries(
    ctx: &SystemContext,
) -> (i32, i32, i32, i32, i32) {
    let mut little_start = 0i32;
    let mut little_end = -1i32;
    let mut big_start = -1i32;
    let mut big_end = -1i32;
    let mut super_core_id = -1i32;
    for c in &ctx.clusters {
        if c.type_name == "小核" {
            little_start = c.start_cpu;
            little_end = c.end_cpu;
        } else if c.type_name == "大核" {
            if big_start == -1 {
                big_start = c.start_cpu;
            }
            big_end = c.end_cpu;
        } else if c.type_name == "超大核" {
            super_core_id = c.start_cpu;
        }
    }
    if little_end < 0 {
        little_end = (ctx.num_cpus as i32) - 1;
    }
    if big_start < 0 {
        big_start = if little_end > 0 { little_end + 1 } else { 0 };
    }
    if big_end < big_start {
        big_end = big_start;
    }
    (little_start, little_end, big_start, big_end, super_core_id)
}

/// 根据线程名和占用率判定分配层级。
/// 关键渲染/UI 线程即使占用率不极端也给大核；GC/JIT 等后台线程即使忙也压小核。
fn thread_tier(name: &str, usage: f64) -> u8 {
    let n = name.to_lowercase();
    // 后台维护线程：即使占用率高也不该抢大核
    let is_background = n.contains("jit")
        || n.contains("heap")
        || n.contains("gc")
        || n.contains("finalizer")
        || n.contains("reference")
        || n.contains("profile")
        || n.contains("daemon")
        || n.contains("cleanup")
        || n.contains("hwui") && n.contains("upload");
    if is_background {
        return TIER_LITTLE;
    }
    // 关键交互/渲染线程：始终最高优先
    let is_critical = n.contains("renderthread")
        || n.contains("render")
        || n.contains("egl")
        || n.contains("glthread")
        || n.contains("libgles")
        || n == "main"
        || n.starts_with("main")
        || n.contains("ui");
    if is_critical && usage >= HIGH_USAGE_THRESHOLD {
        return TIER_SUPER;
    }
    // 按占用率分级
    if usage >= 40.0 {
        TIER_SUPER
    } else if usage >= 20.0 {
        TIER_BIG
    } else if usage >= HIGH_USAGE_THRESHOLD {
        TIER_MID
    } else {
        TIER_LITTLE
    }
}

fn rule_exists(ctx: &mut SystemContext, pkg: &str, thread_name: &str, start_cpu: i32, end_cpu: i32, cpu_usage_percent: f64) -> bool {
    for rule in ctx.historical_rules.iter_mut() {
        if rule.pkg == pkg && rule.thread_name == thread_name && rule.start_cpu == start_cpu && rule.end_cpu == end_cpu {
            rule.cpu_usage_percent = cpu_usage_percent;
            return true;
        }
    }
    false
}

fn add_historical_rule(ctx: &mut SystemContext, pkg: &str, thread_name: &str, start_cpu: i32, end_cpu: i32, cpu_usage_percent: f64) {
    if ctx.historical_rules.len() >= ctx.historical_rules_capacity {
        return;
    }
    if !rule_exists(ctx, pkg, thread_name, start_cpu, end_cpu, cpu_usage_percent) {
        ctx.historical_rules.push(RuleInfo {
            pkg: pkg.to_string(),
            thread_name: thread_name.to_string(),
            start_cpu,
            end_cpu,
            cpu_usage_percent,
        });
    }
}

/// 判断某线程名在历史规则中是否曾持续高占用（用于去抖：单轮偶发高占用不升级）
fn historically_busy(ctx: &SystemContext, pkg: &str, thread_name: &str) -> bool {
    for rule in &ctx.historical_rules {
        if rule.pkg == pkg && rule.thread_name == thread_name && rule.cpu_usage_percent >= 10.0 {
            return true;
        }
    }
    false
}

pub fn generate_rules(
    ctx: &mut SystemContext,
    pkg: &str,
    pkg_threads: &PackageThreads,
    out: &mut String,
    _is_foreground: bool,
    is_system: bool,
) -> (i32, i32) {
    let (_ls, little_end, big_start, big_end, super_core_id) = find_cluster_boundaries(ctx);

    let threads = &pkg_threads.threads;
    let num_threads = pkg_threads.num_threads;

    let mut assigned_super = 0i32;
    let mut assigned_big = 0i32;

    // 收集线程规则，最后统一输出 AppOptR 块格式
    let mut thread_rules: Vec<(String, String)> = Vec::new();

    for i in 0..num_threads {
        let t = &threads[i];
        if t.cpu_usage_percent < HIGH_USAGE_THRESHOLD {
            continue;
        }
        let tier = thread_tier(&t.name, t.cpu_usage_percent);
        let en = escape_thread_name(&t.name);
        // 去抖：单轮偶发高占用但历史不忙的线程，不轻易升到 hp-core
        let need_history = tier == TIER_SUPER;
        let final_tier = if need_history && !historically_busy(ctx, pkg, &t.name) {
            TIER_BIG
        } else {
            tier
        };
        let tier_str = match final_tier {
            TIER_SUPER => {
                assigned_super += 1;
                "hp-core"
            }
            TIER_BIG => {
                assigned_big += 1;
                "p-core,hp-core"
            }
            TIER_MID => {
                assigned_big += 1;
                "p-core"
            }
            _ => "e-core",
        };
        thread_rules.push((en, tier_str.to_string()));
        // 记录历史（用 CPU 号范围，保持内部一致）
        let (sc, ec) = match final_tier {
            TIER_SUPER if super_core_id != -1 => (super_core_id, super_core_id),
            TIER_SUPER => (big_end, big_end),
            TIER_BIG => (big_start, big_end),
            TIER_MID => (big_start, if big_end > big_start + 1 { big_start + 1 } else { big_end }),
            _ => (0, little_end),
        };
        add_historical_rule(ctx, pkg, &t.name, sc, ec, t.cpu_usage_percent);
    }

    // 默认亲和：系统进程给大核，普通应用给小核+中核
    let default_tier = if is_system { "p-core,hp-core" } else { "e-core,p-core" };

    // 输出 AppOptR 兼容块格式
    if thread_rules.is_empty() {
        let _ = writeln!(out, "{}={}", pkg, default_tier);
    } else {
        let _ = writeln!(out, "{}={} {{", pkg, default_tier);
        for (en, tier_str) in &thread_rules {
            let _ = writeln!(out, "\t{}={}", en, tier_str);
        }
        let _ = writeln!(out, "}}");
    }

    // 历史规则输出：曾经高占用但本轮已不忙的线程，作为注释保留建议
    for rule in &ctx.historical_rules {
        if rule.pkg != pkg {
            continue;
        }
        let mut found = false;
        for t in threads {
            if t.name == rule.thread_name && t.cpu_usage_percent >= HIGH_USAGE_THRESHOLD {
                found = true;
                break;
            }
        }
        if !found && rule.cpu_usage_percent >= 1.0 {
            let en = escape_thread_name(&rule.thread_name);
            let _ = writeln!(out, "# 历史: {}={}-{} (历史占用 {:.2}%)", en, rule.start_cpu, rule.end_cpu, rule.cpu_usage_percent);
        }
    }

    (assigned_super, assigned_big)
}
