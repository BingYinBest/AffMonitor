#!/system/bin/sh

MODPATH="${0%/*}"
PROGRAM="Cpu_Aff_Monitor"
CONFIG_FILE="${MODPATH}/配置包名.prop"

export PATH="${PATH}:/data/adb/magisk:/data/adb/ksu/bin:/data/adb/ap/bin"
if command -v busybox >/dev/null 2>&1; then
    sed() { busybox sed "$@"; }
    cmp() { busybox cmp "$@"; }
fi

get_abi() {
    local abi
    abi="$(getprop ro.product.cpu.abi)"
    [ -z "$abi" ] && abi="$(getprop ro.product.cpu.abi64)"
    [ -z "$abi" ] && abi="arm64-v8a"
    echo "$abi"
}

cmp_bin_file() {
    local original_bin_file="${MODPATH}/bin/$(get_abi)/${PROGRAM}"
    local update_bin_file="/data/adb/modules_update/MoniGuard/bin/$(get_abi)/${PROGRAM}"
    [ ! -f "${update_bin_file}" ] && update_bin_file="/data/adb/modules_update/AppOpt_Aloazny_something/bin/$(get_abi)/${PROGRAM}"
    [ ! -f "${update_bin_file}" ] && return
    if ! cmp "${original_bin_file}" "${update_bin_file}" >/dev/null 2>&1; then
        echo "[MoniGuard] 升级 ${PROGRAM} 文件..."
        cp -af "${update_bin_file}" "${original_bin_file}" >/dev/null 2>&1
        chmod 0755 "${original_bin_file}"
    fi
}

desc_modify() {
    local word="$1" file="${MODPATH}/module.prop"
    [ ! -f "$file" ] && return
    [ ! -f "${MODPATH}/backup.prop" ] && cp -f "$file" "${MODPATH}/backup.prop"
    if [ "$word" = "BACKUP" ]; then
        cp -f "${MODPATH}/backup.prop" "$file"
    else
        sed -i "s/^description=.*/description=${word}/" "$file"
    fi
}

BIN_FILE="${MODPATH}/bin/$(get_abi)/${PROGRAM}"
[ ! -f "${BIN_FILE}" ] && BIN_FILE="${MODPATH}/${PROGRAM}"
if [ ! -f "${BIN_FILE}" ]; then
    echo "[MoniGuard] 错误: 未找到 ${PROGRAM} 二进制"
    exit 1
fi

if pgrep -f "${PROGRAM}" >/dev/null 2>&1; then
    echo "[MoniGuard] 正在停止 ${PROGRAM} ..."
    killall -15 "${PROGRAM}" >/dev/null 2>&1
    desc_modify "BACKUP"
    echo "[MoniGuard] 已停止"
else
    echo "[MoniGuard] 正在启动 ${PROGRAM} ..."
    cmp_bin_file
    [ -f "${MODPATH}/service.sh" ] && . "${MODPATH}/service.sh"
    nohup "${BIN_FILE}" --config-file="${CONFIG_FILE}" >/dev/null 2>&1 &
    desc_modify "$(date +'%y-%m-%d %H:%M:%S') 监控已启动，不用时请关闭"
    echo "[MoniGuard] 已启动 (PID: $!)"
fi