#!/system/bin/sh

if [ -n "${MODPATH:-}" ] && [ -f "${MODPATH}/module.prop" ]; then
    :   # 框架已导出有效 MODPATH，直接使用
elif [ -f "${0%/*}/module.prop" ]; then
    MODPATH="${0%/*}"
else
    MODPATH=""
    for _base in /data/adb/modules_update /data/adb/modules; do
        for _dir in "${_base}"/*; do
            [ -f "${_dir}/module.prop" ] || continue
            if grep -q '^id=MoniGuard' "${_dir}/module.prop" 2>/dev/null; then
                MODPATH="${_dir}"
                break 2
            fi
        done
    done
fi

export PATH="${PATH}:/data/adb/magisk:/data/adb/ksu/bin:/data/adb/ap/bin"
if command -v busybox >/dev/null 2>&1; then
    awk() { busybox awk "$@"; }
    sed() { busybox sed "$@"; }
    grep() { busybox grep "$@"; }
fi

ui_print "" >/dev/null 2>&1 || ui_print() { echo "$@"; }

abort() {
    ui_print "$@"
    exit 1
}

grep_prop() {
    local REGEX="s/^$1=//p"; shift
    local FILES="${@}"
    [ -z "$FILES" ] && FILES='/system/build.prop'
    cat "${FILES}" 2>/dev/null | sed -n "${REGEX}" | head -n 1
}

migrate_config() {
    local old_module="/data/adb/modules/AppOpt_Aloazny_something"
    [ ! -d "${old_module}" ] && return
    if [ -f "${old_module}/配置包名.prop" ] && [ ! -f "${MODPATH}/配置包名.prop" ]; then
        cp -af "${old_module}/配置包名.prop" "${MODPATH}/配置包名.prop"
        ui_print "  - 已迁移旧配置包名.prop"
    fi
    if [ -f "${old_module}/运行.sh" ] && [ -f "${MODPATH}/运行.sh" ]; then
        cp -af "${old_module}/运行.sh" "${MODPATH}/运行.sh" 2>/dev/null
        ui_print "  - 已迁移旧运行.sh"
    fi
}

Get_mem_val() {
    local ukey="$1"
    local unit="${2:-MB}"
    local num=$(grep -i "^$ukey:" /proc/meminfo | tr -cd '[:digit:]')
    case "$unit" in
        [Kk][Bb]) echo "$num" ;;
        [Mm][Bb]) echo $((num / 1024)) ;;
        [Gg][Bb]) echo $(((num + 524288) / 1024 / 1024)) ;;
        *) echo "$num" ;;
    esac
}

function get_cpu_core_Info() {
    local mid_count=0 idx=0
    local dir freq_khz freq_mhz related count first last range
    local pol_freq pol_count pol_range pol_num num_pol
    local num min_num max_num arch total c lab r f
    local idx0 idxX freq0 lab0
    local middle_indices mid_count2
    local max_freq max_freq_idx idx_mid freq_mid j idx_m

    for dir in /sys/devices/system/cpu/cpufreq/policy*; do
        [ -f "${dir}/cpuinfo_max_freq" ] || continue
        freq_khz=$(cat "${dir}/cpuinfo_max_freq")
        freq_mhz=$(( freq_khz / 1000 ))
        related=$(cat "${dir}/related_cpus")
        set -- $related
        count=$#
        first=$1
        eval last=\${$#}
        if [ $count -eq 1 ]; then
            range="$1"
        else
            range="${first}-${last}"
        fi
        eval "pol_freq_$idx=\$freq_mhz"
        eval "pol_count_$idx=\$count"
        eval "pol_range_$idx=\$range"
        eval "pol_num_$idx=\${dir##*policy}"
        idx=$((idx+1))
    done
    num_pol=$idx

    if [ $num_pol -eq 0 ]; then
        ui_print "  - CPU核心信息: 无"
        return
    fi

    local i j min_idx val_j_num val_min_num val_order_j val_order_min tmp
    i=0
    while [ $i -lt $num_pol ]; do
        eval "order_$i=\$i"
        i=$((i+1))
    done
    i=0
    while [ $i -lt $((num_pol-1)) ]; do
        min_idx=$i
        j=$((i+1))
        while [ $j -lt $num_pol ]; do
            eval "val_order_j=\$order_$j"
            eval "val_order_min=\$order_$min_idx"
            eval "val_j_num=\$pol_num_$val_order_j"
            eval "val_min_num=\$pol_num_$val_order_min"
            if [ $val_j_num -lt $val_min_num ]; then
                min_idx=$j
            fi
            j=$((j+1))
        done
        if [ $min_idx -ne $i ]; then
            eval "tmp=\$order_$i"
            eval "order_$i=\$order_$min_idx"
            eval "order_$min_idx=\$tmp"
        fi
        i=$((i+1))
    done

    min_num=999999; max_num=-1
    i=0
    while [ $i -lt $num_pol ]; do
        eval "num=\$pol_num_$i"
        [ $num -lt $min_num ] && min_num=$num
        [ $num -gt $max_num ] && max_num=$num
        i=$((i+1))
    done

    if [ $num_pol -eq 1 ]; then
        eval "pol_label_0=\"核心\""
    elif [ $num_pol -eq 2 ]; then
        idx0=-1; idxX=-1
        i=0
        while [ $i -lt $num_pol ]; do
            eval "num=\$pol_num_$i"
            if [ "$num" = "$min_num" ]; then
                idx0=$i
            else
                idxX=$i
            fi
            i=$((i+1))
        done
        eval "freq0=\$pol_freq_$idx0"
        if [ $freq0 -ge 3000 ]; then
            eval "pol_label_$idx0=\"大核\""
            eval "pol_label_$idxX=\"超大核\""
        else
            eval "pol_label_$idx0=\"小核\""
            eval "pol_label_$idxX=\"大核\""
        fi
    else
        i=0
        while [ $i -lt $num_pol ]; do
            eval "pol_label_$i=\"\""
            i=$((i+1))
        done
        i=0
        while [ $i -lt $num_pol ]; do
            eval "num=\$pol_num_$i"
            if [ "$num" = "$min_num" ]; then
                eval "pol_label_$i=\"小核\""
            elif [ "$num" = "$max_num" ]; then
                eval "pol_label_$i=\"超大核\""
            fi
            i=$((i+1))
        done
        mid_count=0
        i=0
        while [ $i -lt $num_pol ]; do
            eval "lab=\$pol_label_$i"
            if [ -z "$lab" ]; then
                eval "middle_indices_$mid_count=\$i"
                mid_count=$((mid_count+1))
            fi
            i=$((i+1))
        done
        if [ $mid_count -eq 1 ]; then
            eval "idx_m=\$middle_indices_0"
            eval "pol_label_$idx_m=\"大核\""
        else
            max_freq=-1; max_freq_idx=-1
            j=0
            while [ $j -lt $mid_count ]; do
                eval "idx_mid=\$middle_indices_$j"
                eval "freq_mid=\$pol_freq_$idx_mid"
                if [ $freq_mid -gt $max_freq ]; then
                    max_freq=$freq_mid
                    max_freq_idx=$idx_mid
                fi
                j=$((j+1))
            done
            j=0
            while [ $j -lt $mid_count ]; do
                eval "idx_mid=\$middle_indices_$j"
                if [ $idx_mid -eq $max_freq_idx ]; then
                    eval "pol_label_$idx_mid=\"大核\""
                else
                    eval "pol_label_$idx_mid=\"降频大核\""
                fi
                j=$((j+1))
            done
        fi
    fi

    arch=""; total=0
    i=0
    while [ $i -lt $num_pol ]; do
        eval "idx=\$order_$i"
        eval "c=\$pol_count_$idx"
        eval "lab=\$pol_label_$idx"
        total=$(( total + c ))
        if [ -z "$arch" ]; then
            arch="${c}${lab}"
        else
            arch="${arch}+${c}${lab}"
        fi
        i=$((i+1))
    done

    ui_print "  - CPU核心配置: ${arch} 共${total}个核心"
    i=0
    while [ $i -lt $num_pol ]; do
        eval "idx=\$order_$i"
        eval "c=\$pol_count_$idx"
        eval "r=\$pol_range_$idx"
        eval "f=\$pol_freq_$idx"
        ui_print "      ${c} Core (${r}) max_freq: ${f} MHz"
        i=$((i+1))
    done

    i=0
    while [ $i -lt $num_pol ]; do
        eval "unset pol_freq_$i pol_count_$i pol_range_$i pol_num_$i pol_label_$i order_$i 2>/dev/null"
        i=$((i+1))
    done
    j=0
    while [ $j -lt $mid_count ]; do
        eval "unset middle_indices_$j 2>/dev/null"
        j=$((j+1))
    done
}

function get_magisk_lite() {
    local until_function=/data/adb/magisk/util_functions.sh
    local Apatch_version=${APATCH_VER_CODE}
    local Ksu_version=${KSU_KERNEL_VER_CODE}
    if [ "${Apatch_version}" != "" ]; then
        ui_print "  - 当前环境非Magisk，疑似Apatch◎${Apatch_version}"
        return
    elif [ "${Ksu_version}" != "" ]; then
        ui_print "  - 当前环境非Magisk，疑似kernelsu◎${Ksu_version}"
        return
    fi
    if grep -q lite_modules "$until_function" >/dev/null 2>&1; then
        ui_print "  - 当前为: Magisk Lite◎$MAGISK_VER_CODE"
    else
        if [ "${MAGISK_VER_CODE}" = "" ]; then
            ui_print "  - 当前环境非Magisk/Ksu/Apatch"
        else
            case "${MAGISK_VER}" in
                *alpha) ui_print "  - 当前为: Magisk Alpha◎$MAGISK_VER_CODE" ;;
                *delta) ui_print "  - 当前为: Magisk Delta◎$MAGISK_VER_CODE" ;;
                *kitsune)
                    local magisk_type="${MAGISK_VER%-*}"
                    if [[ -n $(echo "${magisk_type}" | grep -E '[0-9]{2}\.[0-9]') ]]; then
                        ui_print "  - 当前为: Kitsune Mask◎$MAGISK_VER_CODE"
                    else
                        ui_print "  - 当前为: Kitsune Mask(${magisk_type})◎$MAGISK_VER_CODE"
                    fi
                    ;;
                *-*)
                    local magisk_others="$(echo "${MAGISK_VER##*-}" | tr '[:lower:]' '[:upper:]')"
                    ui_print "  - 当前为: Magisk ${magisk_others}◎$MAGISK_VER_CODE"
                    ;;
                *) ui_print "  - 当前为: Magisk Official◎$MAGISK_VER_CODE" ;;
            esac
        fi
    fi
}

#function check_kernel_schedstat() {
#    local check
#    if [ -f /proc/config.gz ]; then
#        check=$(cat /proc/config.gz | gunzip 2>/dev/null | grep -i 'CONFIG_SCHEDSTATS.*=.*y')
#        if [ -n "${check}" ]; then
#            ui_print "  - 内核 schedstat: 已启用"
#        else
#            ui_print "  - 内核 schedstat: 未检测到（可能仍可用）"
#        fi
#     else
#        ui_print "  - 内核 schedstat: 无法读取 /proc/config.gz"
#    fi
#}

ABI="$(getprop ro.product.cpu.abi)"
[ -z "$ABI" ] && ABI="$(getprop ro.product.cpu.abi64)"
[ -z "$ABI" ] && ABI="arm64-v8a"

ui_print ""
ui_print "**************************************************"
ui_print "  MoniGuard $(grep_prop version "${MODPATH}/module.prop") 安装"
ui_print "**************************************************"

ui_print "  ─ 品牌: $(getprop ro.product.brand)"
ui_print "  ─ 代号: $(getprop ro.product.device)"
ui_print "  ─ 机型: $(getprop ro.product.model)"
ui_print "  ─ Android: $(getprop ro.build.version.release)"
ui_print "  ─ 内核: $(uname -r)"
ui_print "  ─ 运存: $(Get_mem_val 'MemTotal')MB 可用:$(Get_mem_val 'MemAvailable')MB"
ui_print "  ─ Swap: $(Get_mem_val 'SwapTotal')MB 剩余:$(Get_mem_val 'SwapFree')MB"

get_magisk_lite

get_cpu_core_Info

#check_kernel_schedstat

ui_print "  ─ Module version: $(grep_prop version "${MODPATH}/module.prop")"
ui_print "  ─ versionCode: $(grep_prop versionCode "${MODPATH}/module.prop")"
ui_print "  ─ 描述: $(grep_prop description "${MODPATH}/module.prop")"

BIN_FILE="${MODPATH}/bin/${ABI}/Cpu_Aff_Monitor"
if [ ! -f "${BIN_FILE}" ]; then
    ui_print "  [警告] 未找到 ${ABI} 二进制，尝试其他架构..."
    for arch in arm64-v8a armeabi-v7a x86_64 x86; do
        if [ -f "${MODPATH}/bin/${arch}/Cpu_Aff_Monitor" ]; then
            BIN_FILE="${MODPATH}/bin/${arch}/Cpu_Aff_Monitor"
            ui_print "  [使用] ${arch}"
            break
        fi
    done
fi
if [ ! -f "${BIN_FILE}" ]; then
    abort "  [错误] 无可用 Cpu_Aff_Monitor 二进制，安装中止！"
fi

migrate_config

chmod -R 0755 "${MODPATH}/bin" 2>/dev/null
chmod 0755 "${MODPATH}/action.sh" "${MODPATH}/运行.sh" "${MODPATH}/停止.sh" 2>/dev/null
chmod 0644 "${MODPATH}/配置包名.prop" "${MODPATH}/module.prop" 2>/dev/null

rm -f "${MODPATH}/Aloazny.sh" 2>/dev/null

ui_print "--------------------------------------------------"
ui_print "  安装完成"
ui_print "  使用: Magisk Manager 执行 action 切换启停"
ui_print "  或: ${MODPATH}/运行.sh / 停止.sh"
ui_print "**************************************************"