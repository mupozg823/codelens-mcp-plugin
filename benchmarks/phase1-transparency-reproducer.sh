#!/usr/bin/env bash
# Backwards-compatibility shim — the Phase 1 reproducer became the
# multi-phase transparency reproducer after Phase 2 landed.
exec "$(dirname "$0")/transparency-reproducer.sh" "$@"
