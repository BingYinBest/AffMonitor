# MoniGuard

> 模块: 线程亲和监控  
> 作者: Xiao  
> 版本: v43

## 功能

- 监控指定应用/进程的 CPU 亲和性与线程 CPU 占用
- 生成线程分配规则建议
- 支持配置文件指定监控目标（`配置包名.prop`）

## 使用

- 模块默认**不开机自启**，需手动切换
- 在 Magisk Manager 模块页执行 `action` 切换启动/停止
- 或直接运行模块目录下 `运行.sh` / `停止.sh`

## 配置文件

- `配置包名.prop`: 填写要监控的包名，每行一个，`#` 为注释
- 系统进程需加 `=is_system`，如 `surfaceflinger=is_system`
- 监控日志输出到配置文件同目录 `Thread/包名.txt`

## 二进制

- 编译产物 `Cpu_Aff_Monitor` 位于 `bin/<abi>/` 下（arm64-v8a / armeabi-v7a / x86 / x86_64）