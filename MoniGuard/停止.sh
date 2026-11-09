#!/system/bin/sh

PROGRAM="Cpu_Aff_Monitor"
killall -15 "${PROGRAM}" >/dev/null 2>&1
echo "[MoniGuard] 已停止"