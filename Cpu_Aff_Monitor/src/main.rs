mod constants;
mod types;
mod utils;
mod cmd;
mod topapp;
mod procscan;
mod cpuinfo;
mod threads;
mod rules;
mod signals;

use std::fs;
use std::fmt::Write;
use std::rc::Rc;
use std::time::Duration;
use crate::constants::*;
use crate::types::*;
use crate::utils::{affinity_to_string, compare_threads};

fn init_system_context() -> Option<SystemContext> {
    let mut ctx = SystemContext {
        cpu_info: Vec::new(),
        clusters: Vec::new(),
        num_cpus: 0,
        num_clusters: 0,
        cpu_ids: Vec::new(),
        has_m: false,
        has_p: false,
        pid_cache: std::collections::HashMap::new(),
        historical_rules: Vec::new(),
        historical_rules_capacity: MAX_HISTORICAL_RULES,
        thread_states: std::collections::HashMap::new(),
        last_foreground: std::collections::HashMap::new(),
        affinity_initialized: false,
        cached_top_pkg: String::new(),
        last_top_pkg_time: 0,
        permissions_adjusted: false,
        original_selinux_state: 0,
        round: 0,
    };

    cpuinfo::init_cpu_info(&mut ctx);
    signals::setup_signal_handlers();

    Some(ctx)
}

fn save_thread_file(ctx: &SystemContext, config_dir: &str, pkg: &str, pkg_threads: &PackageThreads, is_foreground: bool, is_system: bool, rules_out: &str) {
    let thread_dir = format!("{}/Thread", config_dir);
    if fs::create_dir_all(&thread_dir).is_err() {
        return;
    }
    let file_path = format!("{}/{}.txt", thread_dir, pkg);
    let mut content = String::new();
    let _ = writeln!(content, "CPU 核心簇信息:");
    for (i, c) in ctx.clusters.iter().enumerate() {
        if c.start_cpu == c.end_cpu {
            let _ = writeln!(content, "簇 {}: CPU {}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.capacity, c.max_freq, c.type_name);
        } else {
            let _ = writeln!(content, "簇 {}: CPU {}-{}, 容量 {}, 频率 {}, 类型 {}", i, c.start_cpu, c.end_cpu, c.capacity, c.max_freq, c.type_name);
        }
    }
    content.push_str(rules_out);
    let _ = writeln!(content, "\n应用: {}", pkg);
    let _ = writeln!(content, "应用状态: {}", if is_system { "系统进程（持续统计）" } else { if is_foreground { "前台" } else { "后台（显示最后前台数据）" } });
    let _ = writeln!(content, "CPU占用率(%)\tCPU亲和性\t线程名");
    for t in &pkg_threads.threads {
        let aff = affinity_to_string(&t.affinity, &ctx.cpu_ids);
        let _ = writeln!(content, "{:.2}\t{}\t{}", t.cpu_usage_percent, aff, t.name);
    }
    fs::write(file_path, content).ok();
}

fn print_package_info(ctx: &SystemContext, pkg: &str, pid: i32, pkg_threads: &PackageThreads, is_foreground: bool, is_system: bool) {
    println!("\n应用: {} (PID: {}){}", pkg, pid, if is_system { " [系统进程]" } else { if is_foreground { " [前台]" } else { " [后台]" } });
    println!("应用状态: {}", if is_system { "系统进程（持续统计）" } else { if is_foreground { "前台" } else { "后台（显示最后前台数据）" } });
    println!("CPU占用率(%)\tCPU亲和性\t线程名");
    for t in &pkg_threads.threads {
        let aff = affinity_to_string(&t.affinity, &ctx.cpu_ids);
        println!("{:.2}\t{}\t{}", t.cpu_usage_percent, aff, t.name);
    }
}

fn main() {
    // 崩溃恢复：若上次进程被 SIGKILL 而 SELinux 残留 Permissive，这里还原。
    procscan::recover_selinux_on_startup();

    let mut ctx = match init_system_context() {
        Some(c) => c,
        None => {
            eprintln!("错误: 初始化失败");
            std::process::exit(1);
        }
    };

    let mut pkgs: Vec<(String, bool)> = Vec::new();
    let mut config_file: Option<String> = None;
    let mut config_dir: String = ".".to_string();
    let mut use_config = false;
    let mut last_config_mtime: i64 = -1;

    for arg in std::env::args().skip(1) {
        if arg.starts_with("--config-file=") {
            config_file = Some(arg[14..].to_string());
            if let Some(p) = config_file.as_ref() {
                if let Some(pos) = p.rfind('/') {
                    config_dir = p[..pos].to_string();
                }
            }
            use_config = true;
            break;
        }
    }

    loop {
        if !signals::KEEP_RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        ctx.round = ctx.round.wrapping_add(1);

        if use_config {
            // 仅在配置文件 mtime 变化或首次加载时重新解析 /proc，避免每 5 秒全量扫描。
            let cfg_path = config_file.as_ref().unwrap();
            let mtime = fs::metadata(cfg_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if mtime != last_config_mtime || pkgs.is_empty() {
                last_config_mtime = mtime;
                if let Ok(new_pkgs) = procscan::get_packages_from_config(cfg_path) {
                    pkgs = new_pkgs;
                }
            }
        }

        let sleep_time = Duration::from_secs(UPDATE_INTERVAL);
        let idle_sleep_time = Duration::from_secs(IDLE_INTERVAL);

        if !use_config {
            let mut top_pkg = String::with_capacity(MAX_PKG_LEN);
            match topapp::get_top_app_name(&mut ctx, &mut top_pkg) {
                Some(pkg) if !pkg.is_empty() => {
                    pkgs.clear();
                    pkgs.push((pkg, false));
                }
                _ => {
                    // 没有前台 app，拉长唤醒间隔省电。
                    signals::sleep_fragmented(idle_sleep_time);
                    continue;
                }
            }
        }

        let mut did_scan = false;
        for (pkg, is_sys) in pkgs.iter() {
            let pkg = pkg.as_str();
            let pid = match procscan::get_pid_by_package(&mut ctx, pkg) {
                Some(p) if p > 0 => p,
                _ => continue,
            };

            let is_foreground = if use_config {
                topapp::is_package_foreground(&mut ctx, pkg)
            } else {
                true
            };

            let is_system = *is_sys;

            // 获取本包线程数据。前台/系统直接扫描；后台复用缓存（Rc 引用计数，零拷贝）。
            let data: Rc<PackageThreads> = if is_system || is_foreground {
                did_scan = true;
                match threads::get_threads_by_pid(&mut ctx, pkg, pid) {
                    Some(mut t) => {
                        t.threads.sort_by(|a, b| compare_threads(a, b));
                        let rc = Rc::new(t);
                        // 存入缓存，供该包下次切到后台时复现（BUG-1 修复：按包名索引）。
                        ctx.last_foreground.insert(pkg.to_string(), rc.clone());
                        rc
                    }
                    None => continue,
                }
            } else {
                match ctx.last_foreground.get(pkg).cloned() {
                    Some(rc) if rc.num_threads > 0 => rc,
                    _ => continue,
                }
            };

            // 规则生成只跑一次，结果同时用于 stdout 打印和写文件。
            let mut rules_out = String::new();
            let _ = writeln!(rules_out, "\n分配规则建议:\n#{}", pkg);
            rules::generate_rules(&mut ctx, pkg, &data, &mut rules_out, is_foreground, is_system);
            print!("{}", rules_out);
            print_package_info(&ctx, pkg, pid, &data, is_foreground, is_system);

            if use_config {
                save_thread_file(&ctx, &config_dir, pkg, &data, is_foreground, is_system, &rules_out);
            }
        }

        if did_scan {
            signals::sleep_fragmented(sleep_time);
        } else {
            // 全部包都在后台（用缓存数据），拉长唤醒间隔省电。
            signals::sleep_fragmented(idle_sleep_time);
        }
    }

    procscan::restore_permissions(&ctx);
    println!("\n已退出监控");
}
