#!/usr/bin/env bash
# Allows agents to check UI
#
# Usage:
#   scripts/agent-ui-check.sh
#
# Then open the PNGs under tmp/agent-ui/ (or $SNACK_UI_CAPTURE_DIR).
# This uses offline fixture state — not the live Slack session.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export ICED_TEST_BACKEND="${ICED_TEST_BACKEND:-tiny-skia}"
export SNACK_UI_CAPTURE_DIR="${SNACK_UI_CAPTURE_DIR:-tmp/agent-ui}"

rm -rf "$SNACK_UI_CAPTURE_DIR"
mkdir -p "$SNACK_UI_CAPTURE_DIR"

echo "agent-ui-check: ICED_TEST_BACKEND=$ICED_TEST_BACKEND"
echo "agent-ui-check: SNACK_UI_CAPTURE_DIR=$SNACK_UI_CAPTURE_DIR"
echo "agent-ui-check: running ui_visual tests…"

cargo test --locked ui_visual -- --nocapture

echo
echo "agent-ui-check: captures written to $SNACK_UI_CAPTURE_DIR"
if command -v ls >/dev/null 2>&1; then
  ls -la "$SNACK_UI_CAPTURE_DIR" || true
fi
