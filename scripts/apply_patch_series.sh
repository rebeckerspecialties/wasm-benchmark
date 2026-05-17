#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <repo-dir> <patch-dir>" >&2
  exit 64
fi

repo_dir="$1"
patch_dir="$2"

if [ ! -d "$patch_dir" ]; then
  exit 0
fi

repo_dir="$(CDPATH= cd -- "$repo_dir" && pwd)"
patch_dir="$(CDPATH= cd -- "$patch_dir" && pwd)"

if ! git -C "$repo_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "not a git work tree: $repo_dir" >&2
  exit 1
fi

patch_list="$(mktemp)"

cleanup() {
  rm -f "$patch_list"
}
trap cleanup EXIT INT TERM

find "$patch_dir" -maxdepth 1 -type f -name '*.patch' | sort >"$patch_list"

while IFS= read -r patch; do
  [ -n "$patch" ] || continue

  if git -C "$repo_dir" apply --check "$patch" >/dev/null 2>&1; then
    git -C "$repo_dir" apply "$patch"
    continue
  fi

  if git -C "$repo_dir" apply --reverse --check "$patch" >/dev/null 2>&1; then
    continue
  fi

  echo "patch does not apply cleanly and is not already applied: $patch" >&2
  exit 1
done <"$patch_list"
