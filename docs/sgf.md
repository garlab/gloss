# SGF Go diagrams

`sgf` fences and standalone `.sgf` files use the optional native Go module in
`document-graphics`. No browser, external renderer, new dependency or runtime
download is needed. `--sgf auto` follows the same Ghostty/Kitty hints as other
graphics; `--sgf off` preserves code and `--sgf kitty` explicitly enables a
compatible terminal subject to the usual graphics restrictions.

## Position selection

The default is the final node of the first game's main line, following the first
child at every variation. Collections can be selected with `--sgf-game N`
(one-based). `--sgf-move N` selects the Nth main-line move, counting both `B` and
`W` nodes and passes. Comments and other nodes do not count; SGF's `MN` annotation
does not change this ordinal. Move `0` selects the root, including setup and any
move stored on that root.

Fence attributes override CLI defaults for one board:

````markdown
```sgf game=2 move=1
(;FF[4]GM[1]SZ[9];B[cc])
(;FF[4]GM[1]SZ[9];B[dd];W[ff])
```
````

This displays the second game's first move. Use `move=last` to select its final
position. Unknown, duplicate, malformed and out-of-range
selectors retain source rather than silently showing another position.

The caption identifies the game, move ordinal and main-line selection.
`--show-sgf-source` adds selectable code below successful boards. Other
variations remain in the original record; interactive move/variation navigation
is not implemented. UTF-8 text is required for both Markdown and standalone
files, even though the shared parser also understands some legacy encodings.

## Rendering and fallback

Boards retain setup stones, captures, the last-move mark, and supported SGF
annotations such as labels, shapes and arrows. Reconstruction follows recorded
positions; it is not a competitive-rules or ko validator. Shared-library format,
font and board limits apply. Malformed records, unsupported games or glyphs,
failed rasterization and exhausted budgets keep complete source.

Boards fit the reading column and viewport. Raster scale follows terminal cell
height, with the same 0.25×–4× density range as Mermaid. List markers, quote rails,
cell padding, captions and Kitty transport use the existing image placement
path. The board's wood fill is intentional. Dark/light themes select the shared
renderer palette. Pipes and text/source exports do not parse SGF or allocate
board pixels; exact source copying retains the original text bytes.

## Resources and cancellation

| Resource | Limit |
| --- | --- |
| Source offered to the SGF parser | 256 KiB per record |
| Parsed nodes accepted for replay | 8,192, across the collection |
| New previews per document render | 32, and two seconds of cumulative render work |
| Raster cache | 8 MiB / 32 entries, including source, captions and pixels |
| Returned raster | One million pixels / 8,192 pixels per edge |
| Cell-padded output | Shares the existing 64 MiB placement budget with images and Mermaid |

Source size is checked before parsing; the node limit is checked after the
shared bounded parser finishes. Its own nesting, node/property, annotation and
replay-work limits remain in effect. The time budget stops subsequent uncached
work; it is not a hard deadline interrupting the current native call.

The pager renders off the input thread. Cancellation reaches parsing, main-line
selection, replay and the checks around bounded SVG rasterization. SGF uses no
helper process or OS sandbox. Cache keys include source, selectors, fitting
bounds, scale and theme; failed previews are bounded too. A reload resets work
budgets while retaining cached pixels, and cancellation is never cached.

Named text files in pager mode and standalone diagram input have a 64 MiB read
limit. An oversized SGF record within that input can still be displayed as code.
Standalone images remain on disk until graphics are requested and retain the
existing image read/decode limits.

## Validation and measurement

Rust tests cover main-line and collection selection, passes, root setup,
selector validation, pixel equivalence with the shared renderer, cache identity,
cancellation and source/node limits. Terminal checks cover both themes, narrow
windows, list/quote placement, independent graphics modes, invalid selections,
repeated boards, standalone inputs and exact exports. Pager checks cover SGF
reloads, standalone Mermaid atomic saves and changed standalone image pixels.

Generate benchmark inputs outside the checkout with `bench/generate.py`.
`bench/run.py --renderer mdviewer/terminal-sgf --corpus 'sgf-*'` measures the
feature cases and validates expected image counts. Use `--sgf-mode off` for
disabled rendering and `--sgf-mode legacy` for binaries predating SGF support.
At larger cell sizes, repeated boards can reach the unchanged output budget;
declare their expected count with `--diagram-tiles CORPUS=COUNT`.

### Initial measurements

Compared release builds before (`fe93442`) and after input/SGF support on Apple
M2 Pro / 16 GiB, macOS 26.4.1, Rust 1.99.0. Medians use five fresh processes
after one warm-up, alternating build order, with thin LTO / one codegen unit and
a drained 100×40 PTY, 10×20-pixel cells, 80 content columns and two-cell margins.
Raw measurements and images remain outside the repository; the original
performance baseline is unchanged.

| SGF corpus | Code-only wall / RSS before | Rendering wall / RSS after | Boards | Output after |
| --- | ---: | ---: | ---: | ---: |
| Four fixture boards | 4.4 ms / 3.14 MiB | 11.0 ms / 7.88 MiB | 4 | 69,027 B |
| 19×19 setup with 300 passes | 3.5 ms / 3.11 MiB | 8.9 ms / 8.45 MiB | 1 | 58,907 B |
| 100 repeated boards | 4.0 ms / 3.22 MiB | 37.7 ms / 6.81 MiB | 100 | 1,760,906 B |
| 40 distinct records | 3.6 ms / 3.17 MiB | 47.4 ms / 15.75 MiB | 32 | 459,910 B |

The 40-record case reaches the 32-preview budget; the remaining eight records
retain code. A separate three-sample check after warm-up with `--sgf off` takes
3.3–3.7 ms and 3.19–3.31 MiB across these cases, with the original code output.
At 20×40 cells, the four-board fixture takes 18.4 ms / 13.47 MiB. The repeated
case takes 62.1 ms / 9.63 MiB and emits 55 boards plus 45 source fallbacks after
reaching the unchanged 64 MiB cell-padded output budget.

Unchanged paths remain stable: 10 MiB mixed Markdown **997.1 → 988.8 ms**
(58.25 → 58.33 MiB), 1,000 equations **400.7 → 403.2 ms**, 13 Mermaid fixtures
**47.1 → 47.2 ms**, the dense flowchart **270.1 → 271.2 ms**, and local SVG
**4.53 → 4.55 ms**. Control output byte counts are identical; 28 terminal/export
comparisons also match byte for byte. These differences are within short-run
scheduling and PTY variation. RSS is the reported `wait4` process peak, not a
concurrent process-tree total. Emulator painting, GPU use and scrollback storage
are excluded.

The executable grows by 66,992 bytes (about 65 KiB), from 16,995,360 to
17,062,352 bytes, with no lockfile dependency changes. Validation passes 75 Rust
tests, 49 direct-output PTY tests, 14 pager/watch tests, Clippy and Rust 1.90.
Decoded dark/light board previews were inspected; live emulator painting remains
a separate manual check.
