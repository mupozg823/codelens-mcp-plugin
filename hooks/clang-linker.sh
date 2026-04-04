#!/bin/zsh

set -euo pipefail

clang_bin="${CODELENS_CLANG_BIN:-}"
if [[ -z "${clang_bin}" ]]; then
  clang_bin="$(/usr/bin/xcrun --find clang 2>/dev/null || command -v clang)"
fi

resource_dir="$("$clang_bin" --print-resource-dir)"
rtlib_dir="${resource_dir}/lib/darwin"
sdk_root="${SDKROOT:-$(/usr/bin/xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)}"

args=()
if [[ -n "${sdk_root}" ]]; then
  args+=("-isysroot" "${sdk_root}")
fi
if [[ -d "${rtlib_dir}" ]]; then
  args+=("-L" "${rtlib_dir}")
fi

exec "$clang_bin" "${args[@]}" "$@"
