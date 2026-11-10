use crate::constants::*;
use crate::types::{PackageThreads, RuleInfo, SystemContext, ThreadInfo};
use crate::utils::{appoptr_pattern, default_tier, rule_semantic};
use std::collections::HashMap;
use std::fmt::Write;

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
        // 淘汰最旧一条，避免 Vec 无限增长。
        ctx.historical_rules.remove(0);
    }
    ctx.historical_rules.push(RuleInfo {
        pkg: pkg.to_string(),
        thread_pattern: pattern.to_string(),
        semantic: semantic.to_string(),
        cpu_usage_percent: usage,
    });
}

/// 生成 AppOptR 兼容规则，写入 out。规则按 (模式, 最高档) 分组，输出稳定、紧凑。
pub fn generate_rules(
    ctx: &mut SystemContext,
    pkg: &str,
    pkg_threads: &PackageThreads,
    out: &mut String,
    _is_foreground: bool,
    is_system: bool,
) {
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

    // 2. 按占用率降序（高占用在前），超限裁剪，保持确定性。
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
    let default = default_tier(is_system, ctx.has_p);

    // 4. 输出 AppOptR 兼容块。
    if rules.is_empty() {
        let _ = writeln!(out, "{}={}", pkg, default);
    } else {
        let _ = writeln!(out, "{}={} {{", pkg, default);
        for (pattern, rank, _u) in &rules {
            let sem = rule_semantic(*rank, ctx.has_m, ctx.has_p);
            let _ = writeln!(out, "\t{}={}", pattern, sem);
        }
        let _ = writeln!(out, "}}");
    }

    // 5. 更新历史规则（按 (pkg, pattern) 去重，键不含 CPU 范围，修复 BUG-3）。
    let mut active: Vec<&str> = Vec::with_capacity(rules.len());
    for (pattern, rank, usage) in &rules {
        let sem = rule_semantic(*rank, ctx.has_m, ctx.has_p);
        update_historical(ctx, pkg, pattern, sem, *usage);
        active.push(pattern.as_str());
    }

    // 6. 历史注释：曾经忙但本轮不再活跃的线程，作为建议保留（不参与解析）。
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
