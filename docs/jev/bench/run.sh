#!/usr/bin/env bash
# Jev speed bench — see docs/jev/speed-plan.md
#
# For each task: build a fresh fixture repo, run grok -p twice (Jev off / on),
# record wall-clock and a few stream counters. Does not claim 200x; it produces
# the table that would.
#
# Usage:
#   docs/jev/bench/run.sh              # all tasks, needs `grok` on PATH
#   docs/jev/bench/run.sh --dry-run    # build fixtures only
#   docs/jev/bench/run.sh find-symbol  # one task
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
GROK="${GROK:-grok}"
MODEL="${GROK_BENCH_MODEL:-}"
EFFORT="${GROK_BENCH_EFFORT:-}"
MAX_TURNS="${GROK_BENCH_MAX_TURNS:-24}"
REPEATS="${GROK_BENCH_REPEATS:-1}"
DRY=0
ONLY="${1:-}"

if [[ "${ONLY}" == "--dry-run" ]]; then
  DRY=1
  ONLY="${2:-}"
fi

OUT="${GROK_BENCH_OUT:-$(mktemp -d /tmp/jev-bench-XXXXXX)}"
mkdir -p "${OUT}/fixtures" "${OUT}/runs"
echo "output: ${OUT}" >&2

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1" >&2
    exit 1
  }
}

write_tree() {
  local dir="$1"
  shift
  mkdir -p "${dir}"
  git -C "${dir}" init -q
  git -C "${dir}" config user.email bench@example.com
  git -C "${dir}" config user.name bench
  while [[ $# -gt 0 ]]; do
    local path="$1" body="$2"
    shift 2
    mkdir -p "${dir}/$(dirname "${path}")"
    printf '%s\n' "${body}" >"${dir}/${path}"
  done
  git -C "${dir}" add -A
  git -C "${dir}" commit -q -m fixture
}

# --- fixtures + prompts -----------------------------------------------------

task_trivial_status() {
  write_tree "$1" \
    README.md "hello" \
    src/main.rs "fn main() {}"
  PROMPT="What is the current git status of this repo? One sentence."
}

task_find_symbol() {
  local i
  local args=()
  for i in $(seq 1 19); do
    args+=("src/mod_${i}.rs" "pub fn helper_${i}() { let _ = ${i}; }")
  done
  args+=("src/billing.rs" "pub fn charge_invoice(id: u64) { let _ = id; }")
  write_tree "$1" "${args[@]}"
  PROMPT="Where is charge_invoice defined? Reply with the path only."
}

task_one_line_fix() {
  write_tree "$1" \
    Cargo.toml $'[package]\nname = "t"\nversion = "0.1.0"\nedition = "2021"' \
    src/lib.rs $'pub fn add(a: i32, b: i32) -> i32 { a - b }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() { assert_eq!(super::add(2, 2), 4); }\n}'
  PROMPT="Make the test pass. Do not refactor."
}

task_pick_among_fixes() {
  write_tree "$1" \
    src/lib.rs $'// FIX A: return a+b\n// FIX B: return a*b\n// FIX C: return 4\npub fn add(a: i32, b: i32) -> i32 { a - b }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() { assert_eq!(super::add(1, 2), 3); }\n}'
  PROMPT="The add tests fail. Three comments propose fixes. Apply only the correct one."
}

task_relevance_triage() {
  write_tree "$1" \
    src/a.rs "fn unused() { let invoice = 1; }" \
    src/b.rs "fn other() { let invoice = 2; }" \
    src/c.rs "fn noise() { let invoice = 3; }" \
    src/d.rs "fn more() { let invoice = 4; }" \
    src/e.rs "fn still() { let invoice = 5; }" \
    src/f.rs "fn again() { let invoice = 6; }" \
    src/g.rs "fn pad() { let invoice = 7; }" \
    src/billing.rs $'pub fn charge_invoice(id: u64) -> u64 { id }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn doubles() { assert_eq!(super::charge_invoice(2), 4); }\n}'
  PROMPT="charge_invoice should double its argument. Find the real bug among the invoice hits and fix only that file."
}

task_already_done() {
  write_tree "$1" \
    Cargo.toml $'[package]\nname = "t"\nversion = "0.1.0"\nedition = "2021"' \
    src/lib.rs $'pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() { assert_eq!(super::add(2, 2), 4); }\n}'
  PROMPT="Make the unit test pass."
}

task_multi_file_rename() {
  write_tree "$1" \
    src/lib.rs "pub mod a; pub mod b; pub mod c; pub use a::old_name;" \
    src/a.rs "pub fn old_name() -> u8 { 1 }" \
    src/b.rs "pub fn call() -> u8 { crate::a::old_name() }" \
    src/c.rs "pub fn again() -> u8 { super::old_name() }"
  PROMPT="Rename old_name to new_name everywhere it is defined or called. Do not change behavior."
}

TASKS=(
  trivial-status
  find-symbol
  one-line-fix
  pick-among-fixes
  relevance-triage
  already-done
  multi-file-rename
)

setup_task() {
  case "$1" in
    trivial-status) task_trivial_status "$2" ;;
    find-symbol) task_find_symbol "$2" ;;
    one-line-fix) task_one_line_fix "$2" ;;
    pick-among-fixes) task_pick_among_fixes "$2" ;;
    relevance-triage) task_relevance_triage "$2" ;;
    already-done) task_already_done "$2" ;;
    multi-file-rename) task_multi_file_rename "$2" ;;
    *)
      echo "unknown task: $1" >&2
      exit 1
      ;;
  esac
}

count_stream() {
  local file="$1" needle="$2"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$file" "$needle" <<'PY'
import json, sys
path, needle = sys.argv[1], sys.argv[2]
n = 0
try:
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                if needle in line:
                    n += 1
                continue
            blob = json.dumps(obj, ensure_ascii=False)
            if needle in blob:
                n += 1
except FileNotFoundError:
    pass
print(n)
PY
  else
    grep -c "${needle}" "${file}" 2>/dev/null || echo 0
  fi
}

run_one() {
  local task="$1" mode="$2" repeat="$3" fixture="$4" prompt="$5"
  local home="${OUT}/homes/${task}-${mode}-${repeat}"
  local stream="${OUT}/runs/${task}-${mode}-${repeat}.ndjson"
  mkdir -p "${home}"
  if [[ "${mode}" == "off" ]]; then
    unset JEV_API_KEY TYPESAFE_API_KEY
  fi
  local args=(-p "${prompt}" --output-format streaming-json --max-turns "${MAX_TURNS}" --no-subagents --disable-web-search --permission-mode yolo)
  [[ -n "${MODEL}" ]] && args+=(--model "${MODEL}")
  [[ -n "${EFFORT}" ]] && args+=(--reasoning-effort "${EFFORT}")

  local start end elapsed status
  start="$(date +%s.%N)"
  set +e
  env GROK_HOME="${home}" "${GROK}" "${args[@]}" >"${stream}" 2>"${stream}.err"
  status=$?
  set -e
  end="$(date +%s.%N)"
  elapsed="$(python3 -c "print(round(float('${end}')-float('${start}'), 3))" 2>/dev/null || echo "?")"

  local samples tools asks
  samples="$(count_stream "${stream}" inference_done)"
  if [[ "${samples}" == "0" ]]; then
    samples="$(count_stream "${stream}" tool_call)"
  fi
  tools="$(count_stream "${stream}" '"tool_call"')"
  asks="$(count_stream "${stream}" ask_jev)"

  printf '{"task":"%s","mode":"%s","repeat":%s,"elapsed_s":%s,"exit":%s,"tool_events":%s,"ask_jev_mentions":%s,"stream_hits_inference_done":%s,"stream":"%s"}\n' \
    "${task}" "${mode}" "${repeat}" "${elapsed}" "${status}" "${tools}" "${asks}" "${samples}" "${stream}"
}

if [[ "${DRY}" -eq 0 ]]; then
  need "${GROK}"
  need python3
fi

echo "["
first=1
for task in "${TASKS[@]}"; do
  if [[ -n "${ONLY}" && "${ONLY}" != "--dry-run" && "${task}" != "${ONLY}" ]]; then
    continue
  fi
  fixture="${OUT}/fixtures/${task}"
  rm -rf "${fixture}"
  PROMPT=""
  setup_task "${task}" "${fixture}"
  echo "fixture ${task} -> ${fixture}" >&2
  if [[ "${DRY}" -eq 1 ]]; then
    continue
  fi
  local_repeat=1
  while [[ "${local_repeat}" -le "${REPEATS}" ]]; do
    for mode in off on; do
      [[ "${first}" -eq 1 ]] || echo ","
      first=0
      (cd "${fixture}" && run_one "${task}" "${mode}" "${local_repeat}" "${fixture}" "${PROMPT}")
    done
    local_repeat=$((local_repeat + 1))
  done
done
echo
echo "]"
echo "rows written to stdout; fixtures under ${OUT}/fixtures" >&2
