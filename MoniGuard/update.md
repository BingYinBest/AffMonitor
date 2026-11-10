# MoniGuard 更新日志

## v50
- **智能规则生成**：按线程类别（渲染/后台维护/主线程/通用）识别，双 EWMA（fast 0.6 / slow 0.15）去抖，历史规则按 `(pkg,thread)` 原地更新并输出 `# 历史:` 注释
- **全 CPU 拓扑适配**：支持 1~N 簇，按 `cpu_capacity`/`cpufreq` 分档，语义核 2/3/4+ 档自动折叠（e/p/hp 或 e/m/p/hp）；无 `/sys` 启发式兜底；非连续/离线 CPU 亲和显示修复
- **修复 BUG-1**：`last_foreground`/`thread_states` 由下标改为按包名 `HashMap` 索引，消除后台包缓存张冠李戴
- **修复 BUG-2**：主线程以 `tid==pid` 识别（Android 主线程 comm 为截断包名，非字面 `main`）
- **修复 BUG-3**：历史规则键去除 CPU 范围，命中即原地更新，杜绝重复条目累积到上限
- **修复 BUG-4**：SELinux 崩溃自愈（标记文件 `/data/local/tmp/.moniguard_selinux`，下次启动恢复 Enforcing）
- **修复 BUG-5**：`dumpsys` 输出缓冲 16KB→64KB，命令 3 加 `head -c 30000` 限流
- **修复 BUG-6**：`exec_cmd` 子进程 `setpgid(0,0)` 自成进程组，超时 `kill(-pgid,SIGKILL)` 回收孙进程
- **修复 BUG-7**：亲和显示基于实际升序 `cpu_ids`，修复非连续 CPU 显示错误
- **性能优化**：线程状态/pid 缓存改 HashMap O(1)，comm 名缓存，栈缓冲读 `/proc`，`Rc` 零拷贝，簇拓扑一次性构建，SIGTERM 分片 sleep（<200ms 响应）
- **AppOptR 兼容**：语义核 `e-core`/`m-core`/`p-core`/`hp-core`/`all-core`，包级默认 + 分线程规则 + 块语法；`appoptr_pattern` 屏蔽含 `{}`/`=`/通配的线程名
- 版本：Cargo `0.1.0`→`0.2.0`，module.prop `49`→`50`

## v45-v49（整合迭代）
- v49：多包监控配置、前台应用自动检测（dumpsys，3s 缓存）、线程占用改用 schedstat（纳秒精度）计算、CPU 簇检测（小/大/超大核）与语义档位
- v48：规则分级细化（e/p/hp 档）、历史规则按需分配
- v47：主线程/关键线程识别改进、亲和性显示优化
- v46：SIGTERM/SIGHUP/SIGINT 安全退出与权限恢复
- v45：`exec_cmd` 增加子进程自保护（alarm + poll 超时）

## v44
- 修复 CPU 占用率时间差在 nsec 借位时下溢导致计算错误
- 修复退出线程槽位永不回收导致长期运行后占用率恒 0
- 修复多包监控共用 pkg_idx=0 导致槽位互相挤占
- 修复 exec_cmd 超时不生效（dumpsys 卡死时整个监控挂死），改用 poll+SIGKILL
- 配置包解析从 O(包数×进程数) 优化为单次 /proc 扫描，加 mtime 缓存
- 修复 cmdline 中间 \0 分隔符残留导致 app 进程匹配不可靠
- 内存优化：last_thread_data 从 4MB 固定数组改为按需增长（~32KB），删除未使用的 last_affinity（~16MB）
- 历史规则改为按需分配，pid_cache 原地刷新避免泄漏
