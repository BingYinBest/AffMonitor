# MoniGuard 更新日志

## v44
- 修复 CPU 占用率时间差在 nsec 借位时下溢导致计算错误
- 修复退出线程槽位永不回收导致长期运行后占用率恒 0
- 修复多包监控共用 pkg_idx=0 导致槽位互相挤占
- 修复 exec_cmd 超时不生效（dumpsys 卡死时整个监控挂死），改用 poll+SIGKILL
- 配置包解析从 O(包数×进程数) 优化为单次 /proc 扫描，加 mtime 缓存
- 修复 cmdline 中间 \0 分隔符残留导致 app 进程匹配不可靠
- 内存优化：last_thread_data 从 4MB 固定数组改为按需增长（~32KB），删除未使用的 last_affinity（~16MB）
- 历史规则改为按需分配，pid_cache 原地刷新避免泄漏
