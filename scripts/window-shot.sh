#!/usr/bin/env bash
# Capture one app window, never the whole screen.
#
# Usage: scripts/window-shot.sh <out.png> [owner-substring]
#
# Finds the largest on-screen window whose owning process name contains
# <owner-substring> (default: dbhive), then captures just that window with
# `screencapture -l`. On a Retina display the PNG is 2x the logical size.
set -euo pipefail
out=${1:?usage: window-shot.sh <out.png> [owner-substring]}
owner=${2:-dbhive}

id=$(swift - "$owner" <<'SWIFT'
import CoreGraphics
import Foundation
let want = CommandLine.arguments[1].lowercased()
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
var best: (id: Int, area: Int) = (0, 0)
for w in list {
    let owner = (w[kCGWindowOwnerName as String] as? String ?? "").lowercased()
    let layer = w[kCGWindowLayer as String] as? Int ?? 1
    guard owner.contains(want), layer == 0 else { continue }
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = b["Width"] as? Double ?? 0
    let height = b["Height"] as? Double ?? 0
    let area = Int(width * height)
    if area > best.area { best = (w[kCGWindowNumber as String] as? Int ?? 0, area) }
}
print(best.id)
SWIFT
)
if [ -z "$id" ] || [ "$id" = "0" ]; then
  echo "no on-screen window found for owner matching '$owner'" >&2
  exit 1
fi
mkdir -p "$(dirname "$out")"
screencapture -x -o -l "$id" "$out"
echo "$out (window $id)"
