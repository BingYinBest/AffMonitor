#!/system/bin/sh

MODPATH="${0%/*}"
PROGRAM="Cpu_Aff_Monitor"
CONFIG_FILE="${MODPATH}/配置包名.prop"

get_abi() {
    local abi
    abi="$(getprop ro.product.cpu.abi)"
    [ -z "$abi" ] && abi="$(getprop ro.product.cpu.abi64)"
    [ -z "$abi" ] && abi="arm64-v8a"
    echo "$abi"
}

BIN_FILE="${MODPATH}/bin/$(get_abi)/${PROGRAM}"
[ ! -f "${BIN_FILE}" ] && BIN_FILE="${MODPATH}/${PROGRAM}"
if [ ! -f "${BIN_FILE}" ]; then
    echo "[MoniGuard] 错误: 未找到 ${PROGRAM} 二进制"
    exit 1
fi

killall -15 "${PROGRAM}" >/dev/null 2>&1
sleep 0.2

nohup "${BIN_FILE}" --config-file="${CONFIG_FILE}" >/dev/null 2>&1 &
echo "[MoniGuard] 已启动 (PID: $!)"