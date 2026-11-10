# MoniGuard

> **模块**：Android 应用线程 CPU 亲和监控与规则生成  
> **版本**：v50（versionCode=50）  
> **作者**：Xiao  
> **依赖**：Magisk（或 KernelSU / APatch），运行于 root 环境

MoniGuard 通过读取 `/proc/<pid>/task/<tid>/{stat,schedstat,comm}` 与 `sched_getaffinity`，持续采样目标进程的**每线程 CPU 占用率**与**当前 CPU 亲和性**，并自动生成符合 [AppOptR](https://github.com/BingYinBest/AppOptR) 语法规则的**线程分配建议**（如把高占用渲染线程升到超大核、后台维护线程压到小核），供用户直接粘贴到 AppOptR 配置中。

---

## 功能特性

- **全 CPU 拓扑适配**：自动识别 1~N 个 CPU 簇（按 `cpu_capacity` / `cpufreq` 分档），支持 2 档（e/hp）、3 档（e/p/hp）、4 档及以上（e/m/p/hp）语义核分配；无 `/sys` 时按核心数启发式兜底，兼容离线/非连续 CPU 编号。
- **智能规则生成**：按线程名归类（渲染/交互、后台维护、主线程、通用），结合**双 EWMA**（fast/slow）判断持续占用，生成稳定、去抖、AppOptR 兼容的规则（包级默认 + 分线程规则 + 历史注释）。
- **低开销采样**：线程状态按包名 `HashMap` 索引（O(1)），comm 名缓存（仅新线程或定期重读），栈缓冲读 `/proc`，引用计数零拷贝缓存，`exec_cmd` 带超时与进程组回收。
- **安全退出**：SIGTERM/SIGHUP/SIGINT 均触发分段 sleep（<200ms 响应）干净退出，并自动恢复被临时改写的 SELinux 状态（崩溃后下次启动自愈）。
- **配置文件指定目标**：每行一个包名/进程名，`=is_system` 标记系统进程（持续统计）。

---

## 使用

模块默认**不开机自启**。手动切换：

1. **Magisk Manager** → 模块页 → MoniGuard → 执行 `action`（切换运行/停止）。
2. 或直接运行模块目录下的 `运行.sh` / `停止.sh`。

运行后，监控输出写到配置文件同目录的 `Thread/<包名>.txt`，每 5 秒刷新（全后台空闲时 15 秒）。

---

## 配置文件（配置包名.prop）

```
# 每行一个目标，'#' 开头为注释
# 最多 128 个目标（MAX_PACKAGES）
# 系统进程 / 无法被前台识别的进程，末尾加 "=is_system"
com.coolapk.market
com.tencent.mm
surfaceflinger=is_system
```

- 普通应用：自动识别前台/后台，仅在相关时采样（省电）。
- 系统进程（`=is_system`）：持续统计。
- 输出：`Thread/<包名>.txt`（含 CPU 簇信息、分配规则建议、每线程占用与亲和性）。

---

## 规则输出示例

```
CPU 核心簇信息:
簇 0: CPU 7, 容量 1024, 频率 2995200, 类型 超大核
簇 1: CPU 4-6, 容量 805, 频率 2496000, 类型 大核
簇 2: CPU 0-3, 容量 264, 频率 1804800, 类型 小核

分配规则建议:
com.example=e-core,p-core {
	RenderThread=hp-core
	Jit*=e-core
	com.example=hp-core
}
```

语义核：`e-core`（小）、`m-core`（中）、`p-core`（大）、`hp-core`（超大）、`all-core`（全部）；也可用裸 CPU 范围（如 `0-3`）。符合 AppOptR 的包级/线程级/块语法，可直接粘贴使用。

---

## 二进制

编译产物 `Cpu_Aff_Monitor` 位于 `bin/<abi>/` 下：

| ABI | 目录 |
|-----|------|
| arm64-v8a | `bin/arm64-v8a/` |
| armeabi-v7a | `bin/armeabi-v7a/` |
| x86 | `bin/x86/` |
| x86_64 | `bin/x86_64/` |

源码位于仓库根 `Cpu_Aff_Monitor/`（Rust，单依赖 `libc`）。构建与发版说明见仓库根 `AGENTS.md`。

---

## 更新日志

见 [update.md](update.md)。
