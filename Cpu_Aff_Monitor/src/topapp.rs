use crate::cmd::exec_cmd;
use crate::constants::{CMD_BUFFER_SIZE, MAX_PKG_LEN, TOP_PKG_CACHE_SECS};
use crate::types::SystemContext;
use crate::utils::now_secs;

/// 返回当前前台应用包名（3s 缓存）。命令 3 输出巨大，用 head 限流避免 16KB 截断漏检（BUG-5 修复）。
pub fn get_top_app_name(ctx: &mut SystemContext, buf: &mut String) -> Option<String> {
    if buf.capacity() < MAX_PKG_LEN {
        eprintln!("错误: 无效缓冲区或大小");
        return None;
    }
    let current_time = now_secs();
    if current_time - ctx.last_top_pkg_time < TOP_PKG_CACHE_SECS && !ctx.cached_top_pkg.is_empty() {
        *buf = ctx.cached_top_pkg.clone();
        return Some(buf.clone());
    }

    let mut cmd_output = vec![0u8; CMD_BUFFER_SIZE];
    let commands = [
        "/system/bin/dumpsys window | grep mCurrentFocus",
        "/system/bin/dumpsys window displays | grep -i mFocusedApp",
        "/system/bin/dumpsys activity activities | head -c 30000",
        "/system/bin/dumpsys activity | grep -i mResumedActivity",
    ];
    let keywords = [
        "mCurrentFocus=",
        "mFocusedApp=ActivityRecord{",
        "mActivityComponent=",
        "mResumedActivity:",
    ];

    for i in 0..4 {
        cmd_output.iter_mut().for_each(|b| *b = 0);
        if exec_cmd(commands[i], &mut cmd_output).is_ok() {
            let text = String::from_utf8_lossy(&cmd_output);
            let mut last_match: Option<usize> = None;
            let mut start = 0;
            while let Some(rel) = text[start..].find(keywords[i]) {
                last_match = Some(start + rel);
                start = start + rel + keywords[i].len();
            }
            if let Some(pos) = last_match {
                let mut s = pos + keywords[i].len();
                if i == 0 {
                    while s < text.len() && (text.as_bytes()[s] == b' ' || text.as_bytes()[s] == b'{') {
                        s += 1;
                    }
                    while s < text.len() {
                        if text.as_bytes()[s] == b'u' && (s + 1 < text.len() && text.as_bytes()[s + 1].is_ascii_digit()) {
                            s += 2;
                            while s < text.len() && text.as_bytes()[s].is_ascii_digit() {
                                s += 1;
                            }
                            if s < text.len() && text.as_bytes()[s] == b' ' {
                                s += 1;
                            }
                            break;
                        }
                        s += 1;
                    }
                } else if i == 1 {
                    if let Some(rel) = text[s..].find(' ') {
                        s = s + rel + 1;
                        while s < text.len() && text.as_bytes()[s] != b' ' {
                            s += 1;
                        }
                        s += 1;
                    }
                } else if i == 3 {
                    if let Some(rel) = text[s..].find('{') {
                        s = s + rel + 1;
                        while s < text.len() && text.as_bytes()[s] != b' ' {
                            s += 1;
                        }
                        if s < text.len() && text.as_bytes()[s] == b' ' {
                            s += 1;
                        }
                        while s < text.len() && text.as_bytes()[s] != b' ' {
                            s += 1;
                        }
                        if s < text.len() && text.as_bytes()[s] == b' ' {
                            s += 1;
                        }
                    }
                }
                if s < text.len() {
                    let rest = &text[s..];
                    let end = rest.find('/').or_else(|| rest.find(' '));
                    if let Some(e) = end {
                        let len = e;
                        if len > 0 && len < MAX_PKG_LEN {
                            let pkg = rest[..len].to_string();
                            ctx.cached_top_pkg = pkg.clone();
                            ctx.last_top_pkg_time = current_time;
                            *buf = pkg.clone();
                            return Some(pkg);
                        }
                    }
                }
            }
        }
    }

    buf.clear();
    None
}

pub fn is_package_foreground(ctx: &mut SystemContext, pkg: &str) -> bool {
    let mut top_pkg = String::with_capacity(MAX_PKG_LEN);
    match get_top_app_name(ctx, &mut top_pkg) {
        Some(t) => t == pkg,
        None => false,
    }
}
