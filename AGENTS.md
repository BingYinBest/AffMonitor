# AGENTS.md — MoniGuard / Cpu_Aff_Monitor

Magisk 模块，监控 Android 应用 CPU 亲和性与线程占用，生成 [AppOptR](https://github.com/BingYinBest/AppOptR) 兼容的分配规则。

---

## 项目信息

- **语言**：Rust（`Cpu_Aff_Monitor/`），单依赖 `libc`
- **二进制**：`Cpu_Aff_Monitor`，panic=abort，LTO，strip
- **模块 ID**：`MoniGuard`，作者 `Xiao`
- **当前版本**：48 / versionCode=48
- **编译状态**：四 ABI 0 error 0 warning

## 目录结构

```
AffMonitor/
├── Cpu_Aff_Monitor/src/
│   ├── main.rs       # 主循环：5s 采样，动态间隔（全后台 15s）
│   ├── constants.rs  # 常量
│   ├── types.rs      # ThreadInfo / PackageThreads / SystemContext
│   ├── cmd.rs        # fork+exec 带超时
│   ├── cpuinfo.rs    # CPU 簇拓扑
│   ├── procscan.rs   # /proc 扫描 + 权限
│   ├── topapp.rs     # 前台应用检测（3s 缓存）
│   ├── threads.rs    # 线程占用计算（schedstat 优先）
│   ├── rules.rs      # 规则生成（分级 + AppOptR 格式）
│   ├── signals.rs    # SIGTERM 安全退出
│   └── utils.rs      # 转义等
├── MoniGuard/
│   ├── module.prop
│   ├── action.sh / service.sh / 运行.sh / 停止.sh
│   ├── 配置包名.prop
│   └── bin/<abi>/Cpu_Aff_Monitor
└── Modules_Update/MoniGuard-v*.zip
```

## 构建

NDK 在 `/home/user/android-ndk-r26d/`，四 ABI：

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd Cpu_Aff_Monitor
for t in aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android; do
  cargo build --release --target $t
done
cp target/aarch64-linux-android/release/Cpu_Aff_Monitor ../MoniGuard/bin/arm64-v8a/
cp target/armv7-linux-androideabi/release/Cpu_Aff_Monitor ../MoniGuard/bin/armeabi-v7a/
cp target/i686-linux-android/release/Cpu_Aff_Monitor ../MoniGuard/bin/x86/
cp target/x86_64-linux-android/release/Cpu_Aff_Monitor ../MoniGuard/bin/x86_64/
chmod +x ../MoniGuard/bin/*/Cpu_Aff_Monitor
```

打包：
```bash
cd MoniGuard && zip -r ../Modules_Update/MoniGuard-v48.zip . -x '*/.*'
```

## 规则输出格式（AppOptR 兼容）

```
pkg=e-core,p-core {
	RenderThread=hp-core
	main=hp-core
	BackgroundTask=p-core
}
```

- `e-core` = 小核，`p-core` = 大核，`hp-core` = 超大核，`all-core` = 全部
- 也支持裸 CPU 号：`0-3`、`5-6`
- 分级：≥40% 或 RenderThread/main → `hp-core`；20-40% → `p-core,hp-core`；5-20% → `p-core`
- JIT/GC/Finalizer 等后台线程即使高占用也压 `e-core`
- 历史规则以 `#` 注释输出

## 关键约定

- **优化性能 ≠ 精简功能**，功能删减必须先问用户
- 每改一次代码，四 ABI 都要重新编译
- `cargo check` 必须 0 warning
- 主分支 `master`，只保留一个分支
- 发版流程：改代码 → cargo check → 四 ABI build → cp 到 bin → 升 module.prop → zip → commit → push

## 关键常量

| 常量 | 值 |
|------|-----|
| UPDATE_INTERVAL | 5s（活跃）/ 15s（全后台） |
| HIGH_USAGE_THRESHOLD | 5.0% |
| MAX_PACKAGES | 128 |
| MAX_HISTORICAL_RULES | 2048 |
| CMD_TIMEOUT_SECONDS | 5 |
