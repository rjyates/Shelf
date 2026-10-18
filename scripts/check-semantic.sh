#!/bin/sh
# Check Conventional Commit subjects. Used by .githooks/commit-msg and CI.
set -eu

redo() {
  cat >&2 <<'EOF'

Rejected. Redo this as a semantic commit:

  type(scope): description

Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
Scope is optional. Example: feat(emulator): boot the guest image

Do not bypass this with --no-verify.
EOF
}

is_conventional() {
  printf '%s\n' "$1" | awk '
    BEGIN { ok = 1 }
    {
      if ($0 ~ /^(build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)(\([A-Za-z0-9._/-]+\))?!?: [^[:space:]].*/)
        ok = 0
    }
    END { exit ok }
  '
}

is_git_generated() {
  case "$1" in
    "Merge "*|"fixup! "*|"squash! "*) return 0 ;;
    *) return 1 ;;
  esac
}

note_error() {
  printf '%s\n' "$1" >&2
  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    printf '::error::%s\n' "$1"
  fi
  errors=1
}

check_subject() {
  kind=$1
  subject=$2
  allow_git=$3
  if [ "$allow_git" = yes ] && is_git_generated "$subject"; then
    return 0
  fi
  if is_conventional "$subject"; then
    return 0
  fi
  note_error "$kind rejected: $subject"
}

subject_from_file() {
  sed -n '/^[[:space:]]*#/d; /^[[:space:]]*$/d; p; q' "$1"
}

file=
while [ $# -gt 0 ]; do
  case $1 in
    --file)
      file=$2
      shift 2
      ;;
    *)
      printf 'Unknown argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

errors=0

check_lines() {
  set -f
  old_ifs=$IFS
  IFS='
'
  for subject in $1; do
    [ -n "$subject" ] || continue
    check_subject "Commit subject" "$subject" yes
  done
  IFS=$old_ifs
  set +f
}

if [ -n "$file" ]; then
  check_subject "Commit subject" "$(subject_from_file "$file")" yes
elif [ -z "${PR_TITLE+x}" ] && [ -z "${COMMIT_RANGE:-}" ]; then
  printf 'Pass --file, or set PR_TITLE, or set COMMIT_RANGE.\n' >&2
  exit 2
else
  if [ -n "${PR_TITLE+x}" ]; then
    check_subject "Pull request title" "$PR_TITLE" no
  fi
  if [ -n "${COMMIT_RANGE:-}" ]; then
    start=${COMMIT_RANGE%%..*}
    case $start in
      0000000000000000000000000000000000000000)
        subjects=$(git log -1 --format=%s HEAD)
        ;;
      *)
        subjects=$(git log --format=%s "$COMMIT_RANGE")
        ;;
    esac
    check_lines "$subjects"
  fi
fi

if [ "$errors" -ne 0 ]; then
  redo
  exit 1
fi
