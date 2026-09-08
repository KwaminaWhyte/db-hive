# Baseline measurements

Recorded 2026-09-08, before any migration work. These are the numbers the
rewrite has to beat, and the reason the rewrite was chosen. Everything here
was measured on this machine (Apple silicon, macOS 15.6 / Darwin 25.6),
against a release build, not estimated.

## Method

- Memory is `footprint -p <pid>` `phys_footprint`, which is what Activity
  Monitor reports as "Memory". An earlier pass used `ps` RSS and was wrong:
  RSS counted shared framework pages and overstated the Rust process by
  roughly 3x (90 MB RSS vs 31 MB real).
- 12 samples at 8s intervals, app idle, connected to a local Postgres.
- Bundle sizes are raw (not gzipped) bytes from `dist/assets`, since a
  desktop app loads them off local disk.

## Memory, release build

| Process | phys_footprint |
| --- | --- |
| Rust core (`db-hive`) | 31-36 MB |
| **WebKit WebContent** | **310 MB min, 390 avg, 565 max** |
| WebKit GPU helper | ~42 MB |
| WebKit Networking helper | ~11 MB |
| Total | ~400-650 MB |

The single most important number here: the WebView accounts for roughly
90% of the app, and the Rust core is close to a rounding error. Whatever
else is true, the memory problem lives on the UI side.

The second most important: 310-565 MB idle is far above a bare WKWebView
baseline (~50-80 MB). Several hundred megabytes are being held by the
app's own JS at rest. **This was never explained.** It is worth
understanding before assuming a native GUI automatically fixes it, because
if the cause is retained data (result sets, schema metadata) rather than
the framework, the same mistake ports straight into the new UI.

## Bundle, before and after the split

| | Entry chunk |
| --- | --- |
| Before | 5,540 kB |
| After | 1,061 kB |

Composition of the original 5,540 kB single chunk:

| | Raw |
| --- | --- |
| monaco-editor | 4,136 kB |
| app code | 514 kB |
| recharts | 373 kB |
| misc vendor | 292 kB |
| react + react-dom | 183 kB |
| @tanstack | 152 kB |
| reactflow + dagre | 133 kB |
| @radix-ui (18 primitives) | 106 kB |
| lucide-react | 26 kB |
| react-icons | 16 kB |

Monaco alone was 75% of the JS, imported eagerly at app boot.

## Negative result, recorded deliberately

Splitting the bundle (5,540 kB -> 1,061 kB) produced **no measurable
change in idle memory**. A/B, 12 samples per build:

| Build | WebContent min / avg / max |
| --- | --- |
| Baseline | 310 / 390 / 565 MB |
| After split | 316 / ~340 / 368 MB |

Same ballpark, dominated by GC churn. Bundle size drives startup parse
cost, not steady-state footprint. Anyone later tempted to claim the split
as a memory win should read this table first.

## Codebase size at migration start

| | |
| --- | --- |
| TypeScript / TSX | 41,821 LOC across 170 files |
| Rust | 23,397 LOC across 61 files |
| Database drivers | 7 (postgres, mysql, sqlite, mongodb, redis, sqlserver, turso) |
| Release binary | 30.3 MB stripped, fat LTO |
| App version | 0.21.0-beta |

## Known pre-existing issues found while measuring

- `Cargo.toml` had no `[profile.release]` section at all. Added.
- `AppState::remove_connection` never freed the connection's metadata
  cache. Fixed.
- The accent colour picker in Settings writes `theme.accentColor` and
  nothing anywhere reads it. Still dead as of this commit; the setting is
  purely decorative.

---

# Result: native shell vs the Tauri shell

Measured the same way (`footprint` `phys_footprint`, 12 samples at 8s
intervals, app idle, connected to the same local MySQL), against release
builds of both.

| | Processes | Idle memory |
| --- | --- | --- |
| Tauri + React | 4 (app, WebContent, GPU, Networking) | **~400-650 MB** |
| Native egui | 1 | **111 MB min, 130 avg, 294 max** |

Steady state settles at 111-118 MB. The single 294 MB sample is the first
frames after the schema loads, where the GPU textures are uploaded; it does
not recur.

Component breakdown of the old figure, for comparison:

| Tauri process | phys_footprint |
| --- | --- |
| Rust core | 31-36 MB |
| WebKit WebContent | 310 / 390 / 565 MB (min/avg/max) |
| WebKit GPU helper | ~42 MB |
| WebKit Networking helper | ~11 MB |

So roughly **3.5x to 5x less memory**, and three fewer processes. Binary is
25 MB bundled against the Tauri bundle's 31 MB.

## Honesty about this comparison

- The native shell is not yet at full parity, so it is not loading quite as
  much as the Tauri build was. The structural difference is real regardless:
  the WebView and its two helper processes are gone entirely, and they were
  ~90% of the old footprint.
- The original estimate in this document's first section put a Rust rewrite
  at "best case ~110-120 MB", and that is exactly where it landed. What the
  estimate got wrong was the *baseline*: an early pass used `ps` RSS, which
  understated WebContent badly. Corrected, the saving is far larger than
  first predicted, which is why the rewrite was worth doing.
- The 310-565 MB idle WebContent figure was never explained. It is now moot
  for this app, but if the same retention mistake exists in the native
  UI it would show up here too, so the number above is worth re-checking as
  parity work continues.
