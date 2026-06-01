#!/usr/bin/env bash
set -euo pipefail
# PipeWire note: cpal 0.17.3 does not expose a native 'pipewire' feature; PipeWire is accessible
# via the ALSA or JACK compatibility layer. No libpipewire-sys or libpipewire-dev is required in
# the default build. Check this audit if the cpal version is upgraded to one that adds 'pipewire'.
RESULT=$(cargo tree --edges normal 2>/dev/null | grep -E '(asio-sys v|jack-sys v)' || true)
if [ -n "$RESULT" ]; then
  echo "FFI AUDIT FAILED: forbidden sibling-C crates in default tree (should be feature-gated off):"
  echo "$RESULT"
  exit 1
fi
echo "FFI audit PASSED (asio-sys/jack-sys absent from default tree; OS-boundary crates OK)"
