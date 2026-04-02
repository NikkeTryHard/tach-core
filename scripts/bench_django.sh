#!/usr/bin/env bash
# Django benchmark: tach-core vs pytest (serial) vs pytest-xdist (parallel)
#
# Runs the same 150-test Django ORM suite through all three runners
# and prints a comparison table.
#
# Usage:
#   ./scripts/bench_django.sh              # default: 3 runs, median reported
#   BENCH_RUNS=5 ./scripts/bench_django.sh # 5 runs
#   BENCH_WORKERS=8 ./scripts/bench_django.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENV="${PROJECT_ROOT}/.venv"
PYTHON="${VENV}/bin/python3"
PYTEST="${VENV}/bin/python3 -m pytest"

# tach-core binary: prefer release, fallback to debug
TACH_BINARY="${PROJECT_ROOT}/target/release/tach-core"
if [[ ! -x "$TACH_BINARY" ]]; then
    TACH_BINARY="${PROJECT_ROOT}/target/debug/tach-core"
fi

SUITE_DIR="${PROJECT_ROOT}/tests/benchmark_django"
RUNS="${BENCH_RUNS:-3}"
WORKERS="${BENCH_WORKERS:-4}"

export PYTHONPATH="${PROJECT_ROOT}/tests:${PYTHONPATH:-}"
export DJANGO_SETTINGS_MODULE="django_project.settings"
export PYO3_PYTHON="${PYTHON}"

# -- preflight ---------------------------------------------------------------

if [[ ! -x "$TACH_BINARY" ]]; then
    echo "[FATAL] tach-core binary not found. Run: cargo build --release" >&2
    exit 1
fi

if [[ ! -f "${PYTHON}" ]]; then
    echo "[FATAL] Python venv not found at ${VENV}" >&2
    exit 1
fi

if ! "${PYTHON}" -c "import django" 2>/dev/null; then
    echo "[FATAL] Django not installed in venv. Run: uv pip install django pytest-django pytest-xdist" >&2
    exit 1
fi

echo "==================================================================="
echo "  Django Benchmark: tach-core vs pytest vs pytest-xdist"
echo "==================================================================="
echo ""
echo "  Suite:   ${SUITE_DIR}"
echo "  Runs:    ${RUNS} (median reported)"
echo "  Workers: ${WORKERS} (xdist / tach)"
echo "  Python:  $(${PYTHON} --version 2>&1)"
echo "  Django:  $(${PYTHON} -c 'import django; print(django.VERSION)' 2>&1)"
echo "  tach:    ${TACH_BINARY}"
echo ""

# -- timing helpers -----------------------------------------------------------

collect_times() {
    local -n arr=$1
    local cmd=$2
    local label=$3

    echo -n "  ${label}: "
    for ((i=1; i<=RUNS; i++)); do
        local start end elapsed
        start=$(date +%s%N)
        eval "$cmd" > /dev/null 2>&1 || true
        end=$(date +%s%N)
        elapsed=$(( (end - start) / 1000000 ))
        arr+=("$elapsed")
        echo -n "${elapsed}ms "
    done
    echo ""
}

median() {
    local sorted=($(printf '%s\n' "$@" | sort -n))
    local count=${#sorted[@]}
    local mid=$(( count / 2 ))
    if (( count % 2 == 0 )); then
        echo $(( (sorted[mid-1] + sorted[mid]) / 2 ))
    else
        echo "${sorted[$mid]}"
    fi
}

# -- warm up (first run seeds DB, populates caches) ---------------------------

echo "[warmup] pytest serial (cold)..."
${PYTEST} "${SUITE_DIR}" -q --tb=no -p no:randomly > /dev/null 2>&1 || true

echo "[warmup] tach-core (cold)..."
"${TACH_BINARY}" --no-isolation "${SUITE_DIR}" > /dev/null 2>&1 || true

echo ""

# -- collect timings ----------------------------------------------------------

echo "[bench] Collecting ${RUNS} runs each..."
echo ""

declare -a PYTEST_SERIAL_TIMES=()
declare -a PYTEST_XDIST_TIMES=()
declare -a TACH_TIMES=()

collect_times PYTEST_SERIAL_TIMES \
    "${PYTEST} ${SUITE_DIR} -q --tb=no -p no:randomly -p no:xdist" \
    "pytest (serial)"

collect_times PYTEST_XDIST_TIMES \
    "PYTHONPATH=${PROJECT_ROOT}/tests:${PYTHONPATH:-} ${PYTEST} ${SUITE_DIR} -q --tb=no -p no:randomly -n ${WORKERS}" \
    "pytest-xdist (${WORKERS}w)"

collect_times TACH_TIMES \
    "${TACH_BINARY} --no-isolation ${SUITE_DIR}" \
    "tach-core"

echo ""

# -- compute medians ----------------------------------------------------------

MED_SERIAL=$(median "${PYTEST_SERIAL_TIMES[@]}")
MED_XDIST=$(median "${PYTEST_XDIST_TIMES[@]}")
MED_TACH=$(median "${TACH_TIMES[@]}")

# -- speedup calculations ----------------------------------------------------

if (( MED_SERIAL > 0 )); then
    SPEEDUP_XDIST_VS_SERIAL=$(echo "scale=2; ${MED_SERIAL} / ${MED_XDIST}" | bc)
    SPEEDUP_TACH_VS_SERIAL=$(echo "scale=2; ${MED_SERIAL} / ${MED_TACH}" | bc)
else
    SPEEDUP_XDIST_VS_SERIAL="N/A"
    SPEEDUP_TACH_VS_SERIAL="N/A"
fi

if (( MED_XDIST > 0 )); then
    SPEEDUP_TACH_VS_XDIST=$(echo "scale=2; ${MED_XDIST} / ${MED_TACH}" | bc)
else
    SPEEDUP_TACH_VS_XDIST="N/A"
fi

# -- results table ------------------------------------------------------------

echo "==================================================================="
echo "  RESULTS (150 Django ORM tests, median of ${RUNS} runs)"
echo "==================================================================="
echo ""
printf "  %-24s %10s %10s\n" "Runner" "Time (ms)" "vs serial"
printf "  %-24s %10s %10s\n" "------------------------" "----------" "----------"
printf "  %-24s %10d %10s\n" "pytest (serial)"         "$MED_SERIAL" "1.00x"
printf "  %-24s %10d %10sx\n" "pytest-xdist (${WORKERS}w)"    "$MED_XDIST"  "$SPEEDUP_XDIST_VS_SERIAL"
printf "  %-24s %10d %10sx\n" "tach-core"               "$MED_TACH"   "$SPEEDUP_TACH_VS_SERIAL"
echo ""
printf "  tach-core vs xdist: %sx faster\n" "$SPEEDUP_TACH_VS_XDIST"
echo ""
echo "  Raw times:"
printf "    pytest serial:  %s\n" "${PYTEST_SERIAL_TIMES[*]}ms"
printf "    pytest-xdist:   %s\n" "${PYTEST_XDIST_TIMES[*]}ms"
printf "    tach-core:      %s\n" "${TACH_TIMES[*]}ms"
echo ""
echo "==================================================================="
