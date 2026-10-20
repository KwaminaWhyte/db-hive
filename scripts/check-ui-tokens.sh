#!/usr/bin/env bash
#
# UI token ratchet. Components must take colours and font sizes from the Theme
# global, never from literals (docs/ui-guidelines.md section 2).
#
# Counts literal colours and px font sizes in crates/dbhive-slint/ui, outside the
# generated theme, the icon table and the spikes. Fails when the count goes UP
# against scripts/ui-token-baseline.txt. Lower it by fixing literals, then run
# `scripts/check-ui-tokens.sh --update` to ratchet the baseline down.
#
#   scripts/check-ui-tokens.sh            check
#   scripts/check-ui-tokens.sh --list     print every offending line
#   scripts/check-ui-tokens.sh --update   write the current count as the baseline
set -uo pipefail
cd "$(dirname "$0")/.."

UI=crates/dbhive-slint/ui
BASELINE=scripts/ui-token-baseline.txt

offences() {
  grep -rnE '#[0-9a-fA-F]{3,8}\b|rgba?\(|font-size: *[0-9]+px' --include='*.slint' "$UI" \
    | grep -vE "^$UI/spikes/|/theme\.slint:|/icons\.slint:"
}

count=$(offences | wc -l | tr -d ' ')

case "${1:-}" in
  --list) offences; echo "total: $count"; exit 0 ;;
  --update) echo "$count" > "$BASELINE"; echo "baseline set to $count"; exit 0 ;;
esac

base=$(cat "$BASELINE" 2>/dev/null || echo 0)
if [ "$count" -gt "$base" ]; then
  echo "UI token check FAILED: $count literal colours or px font sizes, baseline is $base."
  echo "Use Theme.* tokens instead. See docs/ui-guidelines.md section 2. Offences:"
  offences
  exit 1
fi
echo "UI token check ok: $count (baseline $base)"
[ "$count" -lt "$base" ] && echo "Lower than baseline: run scripts/check-ui-tokens.sh --update to lock it in."
exit 0
