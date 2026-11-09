#!/system/bin/sh

MODPATH="${0%/*}"
PROGRAM="Cpu_Aff_Monitor"

# 设置二进制与脚本权限
chmod -R 0755 "${MODPATH}/bin" 2>/dev/null
chmod 0755 "${MODPATH}/action.sh" "${MODPATH}/运行.sh" "${MODPATH}/停止.sh" 2>/dev/null

# 配置文件路径
# config_file="${MODPATH}/配置包名.prop"

# 开机自启（默认关闭，取消注释即可启用）
# ABI="$(getprop ro.product.cpu.abi)"
# [ -z "$ABI" ] && ABI="arm64-v8a"
# BIN="${MODPATH}/bin/${ABI}/${PROGRAM}"
# [ -f "$BIN" ] && nohup "$BIN" --config-file="$config_file" >/dev/null 2>&1 &