#!/usr/bin/env bash
# Print "x y width height id" for the largest on-screen window whose owning
# process name contains <owner-substring> (default: dbhive).
#
# Used with scripts/window-shot.sh and cliclick to drive the real GPU window.
set -euo pipefail
owner=${1:-dbhive}
swift - "$owner" <<'SWIFT'
import CoreGraphics
import Foundation
let want = CommandLine.arguments[1].lowercased()
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
var best: (id: Int, x: Double, y: Double, w: Double, h: Double, area: Double) = (0, 0, 0, 0, 0, 0)
for w in list {
    let owner = (w[kCGWindowOwnerName as String] as? String ?? "").lowercased()
    let layer = w[kCGWindowLayer as String] as? Int ?? 1
    guard owner.contains(want), layer == 0 else { continue }
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let x = b["X"] as? Double ?? 0
    let y = b["Y"] as? Double ?? 0
    let width = b["Width"] as? Double ?? 0
    let height = b["Height"] as? Double ?? 0
    let area = width * height
    if area > best.area { best = (w[kCGWindowNumber as String] as? Int ?? 0, x, y, width, height, area) }
}
if best.id == 0 { exit(1) }
print("\(Int(best.x)) \(Int(best.y)) \(Int(best.w)) \(Int(best.h)) \(best.id)")
SWIFT
