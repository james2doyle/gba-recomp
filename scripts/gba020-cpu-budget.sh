# Shared POSIX shell helpers for CPU limits in GBA-020 gate scripts.

gba020_normalize_budget_value() {
    gba020_raw_value=$1
    case "$gba020_raw_value" in
        ''|*[!0-9]*) return 1 ;;
    esac
    while [ "${gba020_raw_value#0}" != "$gba020_raw_value" ]; do
        gba020_raw_value=${gba020_raw_value#0}
    done
    case "$gba020_raw_value" in
        1|2|3|4|5|6|7|8|9|10) ;;
        *) return 1 ;;
    esac
    GBA020_NORMALIZED_VALUE=$gba020_raw_value
}

gba020_detect_cpu_budget() {
    gba020_os=$1
    gba020_arch=$2
    gba020_detected_raw=
    GBA020_CPU_DETECTOR=fallback-one
    GBA020_CPU_FALLBACK=1

    case "$gba020_os/$gba020_arch" in
        Darwin/arm64)
            GBA020_CPU_DETECTOR=sysctl-hw.logicalcpu
            if command -v sysctl >/dev/null 2>&1; then
                gba020_detected_raw=$(sysctl -n hw.logicalcpu 2>/dev/null) || gba020_detected_raw=
            fi
            ;;
        Linux/x86_64)
            GBA020_CPU_DETECTOR=nproc
            if command -v nproc >/dev/null 2>&1; then
                gba020_detected_raw=$(nproc 2>/dev/null) || gba020_detected_raw=
            fi
            ;;
        *)
            gba020_detected_raw=
            ;;
    esac

    case "$gba020_detected_raw" in
        ''|*[!0-9]*)
            GBA020_DETECTED_CPUS=unknown
            GBA020_AUTOMATIC_BUDGET=1
            GBA020_CPU_DETECTOR=fallback-one
            return
            ;;
    esac
    while [ "${gba020_detected_raw#0}" != "$gba020_detected_raw" ]; do
        gba020_detected_raw=${gba020_detected_raw#0}
    done
    if [ -z "$gba020_detected_raw" ]; then
        GBA020_DETECTED_CPUS=unknown
        GBA020_AUTOMATIC_BUDGET=1
        GBA020_CPU_DETECTOR=fallback-one
        return
    fi

    GBA020_CPU_FALLBACK=0
    GBA020_DETECTED_CPUS=$gba020_detected_raw
    if [ "${#gba020_detected_raw}" -gt 2 ]; then
        GBA020_AUTOMATIC_BUDGET=10
    elif [ "$gba020_detected_raw" -gt 10 ]; then
        GBA020_AUTOMATIC_BUDGET=10
    else
        GBA020_AUTOMATIC_BUDGET=$gba020_detected_raw
    fi
}

gba020_choose_effective_budget() {
    if [ "${GBA020_CPU_BUDGET+x}" = x ]; then
        if ! gba020_normalize_budget_value "$GBA020_CPU_BUDGET"; then
            echo "GBA020_CPU_BUDGET must be a decimal integer from 1 to 10" >&2
            return 1
        fi
        if [ "$GBA020_NORMALIZED_VALUE" -gt "$GBA020_AUTOMATIC_BUDGET" ]; then
            echo "GBA020_CPU_BUDGET cannot exceed the detected automatic budget ($GBA020_AUTOMATIC_BUDGET)" >&2
            return 1
        fi
        GBA020_EFFECTIVE_BUDGET=$GBA020_NORMALIZED_VALUE
        GBA020_BUDGET_OVERRIDE=$GBA020_NORMALIZED_VALUE
    else
        GBA020_EFFECTIVE_BUDGET=$GBA020_AUTOMATIC_BUDGET
        GBA020_BUDGET_OVERRIDE=none
    fi
}
