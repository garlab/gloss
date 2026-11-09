# Interactive pager

`--pager` is opt-in. `--watch` adds debounced file reloads and implies the pager.
Both use the existing renderer, themes, margins, reading width, and graphics
fallbacks. Commands and export behavior are documented in the [README](../README.md).
The current implementation targets Unix terminals, including macOS. It requires
a controlling terminal for keys and uses an alternate screen, leaving ordinary
scrollback output available without either flag.

## Layout, images, and copying

The renderer writes into a recorder that understands only its own ANSI and Kitty
vocabulary. Text is already sanitized. Completed styled rows and base64 graphics
payloads go into anonymous temporary files, with lightweight row and image
indexes in memory. A small rolling row window handles the renderer's reserved
space and cursor rewinds. Bounded math and Mermaid raster caches can survive
reloads; the pager does not retain every raster or the full ANSI output in RAM. Temporary files are removed automatically when their descriptors close.

Only visible rows are replayed. Kitty image numbers identify uploads owned by
this view; adjacent scroll positions reuse visible image data. Cropping happens
in source pixels so partial equations and diagrams retain their scale. Native
math placements preserve their within-cell pixel offsets when clipped at either
viewport edge. Leaving
the viewport frees an image's terminal data. Reload, resize, suspension, and exit
clear owned uploads. No global image-delete command is sent. Text rows are erased
individually because a full display erase would also evict Kitty graphics.

Mouse tracking stays disabled. Native selection can copy visible text, captions,
and optional TeX/Mermaid source. Pixels do not encode selectable source. Exact
copying still uses independent `--output source` exports, including to `pbcopy`.
Search is literal and case-sensitive over rendered text, including visible source
fallbacks. It wraps in both directions and runs in bounded slices so navigation
can interrupt it. It does not search raster contents or across wrapped rows.

## Reloading and input

One background thread renders a complete candidate page. At most one request runs
and one replacement waits; newer requests cancel stale work and replace the
pending request. Publication swaps the whole page.

When nothing is shown yet, the first render publishes a provisional page first:
it records whole blocks until two screens of rows exist, stops between blocks,
and then renders the complete page from the start. The provisional rows equal
the complete page's first rows, and the footer marks the row count with `+`
while `rendering`. Its last row is not treated as the document's end, so a view
there keeps its position instead of following to the complete page's end. The
first screen's equations and diagrams are cache hits in the second pass. Reloads
and resizes keep the previous page instead, so they never show a truncated
document. A first block taller than the preview, such as a long table, delays
the preview until it is complete. The previous good page remains
available if a watch read or render fails. Resize reflow temporarily clears an
incompatible viewport rather than replaying rows at the wrong width.

Watch polls the named path's device, inode, size, and modification/change times
every 250 ms. A change must remain stable for a subsequent poll before loading,
so ordinary edits appear after roughly 250–500 ms plus rendering time. This
detects atomic replacement and disappearance/recreation without directory-wide
subscriptions. LaTeX documents also poll the files their last successful render
included, up to 63; a change there forces a render even though the named file's
bytes are unchanged. Files that did not exist at that render are not polled. Unchanged text bytes skip rendering. Standalone Mermaid/SGF files
retain their input format across reloads. Standalone images re-read their pixels
after metadata changes; an unchanged filename does not suppress rendering.
There is no content hashing,
image-directory polling, or helper process while idle. Manual `r` also refreshes
local images. File reads use nonblocking opens and reject nonregular files, so a
replacement FIFO cannot block the input loop.

Rows retain source block anchors. Reload maps anchors past a changed prefix or
suffix where possible; resize preserves the relative row within the source
block. Large edits inside that block can move the view. A view at the bottom
follows appended content. This is position preservation, not a persistent editor
cursor or a full document diff.

Input comes from `/dev/tty`, independently of document stdin. Escape sequences,
UTF-8 keys, and bracketed paste have bounded decoding; bracketed paste is ignored.
The terminal guard restores termios, cursor visibility, hyperlinks, and the
previous screen on quit, errors, and unwinding. SIGINT, SIGTERM, SIGHUP and SIGQUIT
request cleanup. Ctrl-Z/SIGTSTP restore the screen before suspending; resumption
reenters and redraws. Uncatchable termination cannot execute application cleanup.
Cancelling rendering uses a nonretryable I/O error: `Write::write_all` would retry
`Interrupted` indefinitely. Cleanup restores the terminal before joining the active renderer. Its cancellation
signal reaches the Mermaid worker supervisor, so stale requests and quit kill
and reap an in-flight diagram instead of waiting for its reply timeout.

Reload and reflow reuse the existing math, Mermaid and SGF raster caches, with the
same byte/entry limits. Each render restarts equation numbering and request/time/
output budgets. Cache keys include the content, fitting bounds and appearance;
changed bounds or colors cannot reuse incorrectly fitted pixels. Local image
loaders are fresh on every render. The Mermaid helper exits after each render,
including errors and cancellation, so no helper remains alive while idle.
A nonblocking socket pair wakes the input loop when a render completes, avoiding
an extra input-poll delay without increasing the idle polling rate.

## Resource limits

| Resource | Limit |
| --- | --- |
| UTF-8 text input (Markdown, code, Mermaid or SGF) | 64 MiB |
| Row and graphics spools combined | 256 MiB |
| Indexed rendered rows | 1,000,000 |
| Graphics records | 32,768 |
| Encoded text and search text per row | 256 KiB |
| Editable row window | Content viewport height + 16 rows, at most 8 MiB of row data |
| Viewport geometry | At most 4,096 columns and rows |
| Decoded graphics retained in the terminal | 64 MiB of visible uploads |
| Search query | 256 UTF-8 bytes |
| Search work between input checks | Up to 512 rows or 5 ms |
| Background rendering | One active request and one replaceable pending request |
| Math cache retained between renders | 8 MiB of compressed pixels / 4,096 entries |
| Mermaid cache retained between renders | 8 MiB / 32 entries |
| SGF cache retained between renders | 8 MiB / 32 entries |

The existing math, image, code, table, Mermaid and SGF budgets apply too. Full source,
syntax projections, parser storage, and large individual blocks still scale with
input. Keeping a displayed page while building a replacement can temporarily use
two page spools. These are component limits, not an overall RSS guarantee.

Pager storage exhaustion reports an error instead of truncating content; watch
retains the last good page. Direct output and exports remain alternatives for
larger files, with their existing resource policies. The pager initializes no
threads, signal handlers, temporary files, or polling on those paths.

## Validation

Run `python3 tests/pager_smoke.py target/debug/gloss` after building. Its
controlling PTY exercises piped document input, navigation, Unicode search,
resize, graphics payloads and clipping, upload reuse/eviction, atomic saves,
read failures/recovery, native-selection policy, and terminal restoration.
The screen checker validates protocol and cell rectangles, not a terminal's GPU
or font rendering. Keep real Ghostty/Kitty visual and selection checks separate.

## Measurements

Measured on Apple M2 Pro / 16 GiB, macOS, Rust 1.99, release builds with thin LTO
and stripped symbols. One warm-up and three fresh measured processes per case;
100×40 PTY, 10×20-pixel cells, warm filesystem caches. The comparison builds use
the same rendering backends. Raw results and large generated inputs remain
outside the repository; the fixed performance baseline is unchanged.

| 10 MiB mixed document | Before pager changes | After pager changes |
| --- | ---: | ---: |
| Direct styled TeX output, median wall time | 1.081 s | 1.148 s |
| Direct styled TeX output, median peak RSS | 58.20 MiB | 58.19 MiB |
| Plain pipe output, median wall time | 94.1 ms | 100.3 ms |
| Plain pipe output, median peak RSS | 54.75 MiB | 54.84 MiB |

Direct-path timing changed by about 6–7% in these samples, below the agreed
regression-review thresholds; output byte counts are identical. The executable
grew from 16,775,680 to 16,978,544 bytes, about 0.19 MiB or 1.2%. The two new direct
dependencies, `signal-hook` and `tempfile`, are Unix-only.

The pager's median time to a complete usable page was **1.333 s**, with **85.47
MiB** median peak RSS. The initial loading screen appeared in 7–8 ms. Navigation
over the finished page took 0.10–0.18 ms per command in the drained PTY; quitting
took 3.8–7.9 ms. Extra memory covers retained source and row/image indexes; image
pixels and completed rendered text are not retained for the entire document.

For 100 repeated Mermaid fences, watch mode reached a complete page in **63.1
ms**, with **10.33 MiB** median reported viewer RSS. Page jumps requiring uploads
took 1.2–1.8 ms. Each of three subsequent 3-second idle intervals emitted **zero
bytes**, retained **zero helper processes**, and added **0.00 s** of CPU at `ps`'s
hundredth-of-a-second resolution. This RSS is not a concurrent process-tree
measurement of the active Mermaid worker.

PTY timings exclude emulator decoding, GPU work, selection, and compositor
latency. These samples are references, not portable performance guarantees.
Repeat using the existing externally generated corpora:

```sh
python3 bench/pager.py --binary target/release/gloss \
  --input "$TMPDIR/gloss-bench/corpora/mixed-10m.md"
python3 bench/pager.py --binary target/release/gloss \
  --input "$TMPDIR/gloss-bench/corpora/mermaid-repeated-100.md" \
  --watch-seconds 3
```

The script reports readiness, individual navigation latencies, quit time,
process-reported RSS, CPU, output bytes, and idle helper count as JSON. Use
`bench/run.py` for direct-output comparisons and separate process-tree sampling.

## Reload and reflow performance

A later comparison against `885be54` measures retained graphics caches and
completion wake-ups together, using published `document-graphics` 0.1.3 and
`katex-native` 0.1.0. Hardware/profile/PTY geometry are as above. Each binary had
one warm-up and five alternating fresh measured sessions. Each session performed
eight manual reloads and three 100→60→100-column resize cycles. Reload figures
are medians of per-session medians; warmed resize figures exclude the first
cycle. They include page construction, upload and status output to the drained
PTY, but exclude terminal drawing. The 200 ms geometry check and 250–500 ms
watch debounce remain separate from cache rendering time.

| Input / operation | Before | After |
| --- | ---: | ---: |
| 32 distinct inline fractions: first complete page | 59.4 ms | 11.7 ms |
| 32 fractions: manual reload | 55.3 ms | 1.29 ms |
| 32 fractions: warmed resize | 209 ms | 211 ms |
| 32-node / 60-edge flowchart: first complete page | 278 ms | 265 ms |
| Flowchart: manual reload | 271 ms | 2.96 ms |
| Flowchart: warmed resize | 425 ms | 215 ms |

The input loop previously waited up to 50 ms to discover a completed render.
The completion notification removes that floor without faster idle polling.
Cache reuse avoids layout/rasterization on hits; encoding and page recording
still run. Documents exceeding the existing cache capacity can evict entries
and will not receive the same gains.

Separate stress sessions performed 100 reloads and 50 resize cycles per input.
Viewer RSS settled at about 8.4 MiB for equations and 17.8 MiB for the flowchart,
with the final 50 samples spanning 8.36–8.41 MiB / a flat 17.80 MiB.
Peak viewer RSS was 8.44 / 19.70 MiB.
Each final one-second idle period retained zero helpers and recorded 0.00 seconds
of CPU at the sampling tool's hundredth-second resolution. These are viewer RSS
figures, not concurrent process-tree or terminal memory measurements. Cache
entries for the second fitting width account for expected warm-up growth.

The executable grew by 16,720 bytes (about 0.10%), with no dependency changes.
Direct 10 MiB styled-text rendering measured 959 / 971 ms with 57.91 / 58.17 MiB
peak RSS; plain-pipe rendering measured 97.2 / 96.3 ms. Those small differences
are not evidence of a meaningful direct-path change. Direct text and Mermaid
output byte counts are unchanged. The fixed initial baseline remains untouched.

The runner now records reload/resize latency and resident samples:

```sh
python3 bench/pager.py --binary target/release/gloss \
  --input "$TMPDIR/gloss-bench/corpora/math-100.md" \
  --reloads 8 --resize-cycles 3 --watch-seconds 1
```

Use a working set of at most 4,096 math entries / 32 diagram entries within the
8 MiB budgets to measure warm-cache behavior; also measure larger working sets
to observe eviction. Changing fitting bounds produces distinct cache entries.

### First-screen preview

`first_page_ms` in `bench/pager.py` is the first frame showing document rows;
`ready_ms` is the first complete page. Five sessions per binary, compared against
the previous commit, with medians:

| Input | First page before / after | Complete page before / after |
| --- | ---: | ---: |
| LaTeX corpus `body.tex` | 66 / 13 ms | 66 / 66 ms |
| Markdown, 3,500 formulas (described below) | 622 / 17 ms | 622 / 625 ms |

Rendering the first screen twice costs little because its equations are cached
by then. Reload, resize, and peak RSS were unchanged within noise. Direct output
does not use previews.

### Compressed equation cache

The math cache later kept compressed pixels instead of RGBA, raised its entry
limit from 64 to 4,096 within the same 8 MiB, and gained a hash index.
Equations are compressed once after rasterization, which transmission already
required, so cache hits skip both rasterization and compression. Hardware,
profile, and PTY geometry are as above; three fresh sessions per binary, each
with four manual reloads, compared against the previous commit:

| Input | First page before / after | Manual reload before / after | Peak viewer RSS before / after |
| --- | ---: | ---: | ---: |
| LaTeX corpus `body.tex` (374 KB, 474 formulas; see [bench/latex](../bench/latex)) | 61 / 59 ms | 57 / 26 ms | 12.4 / 11.6 MiB |
| Markdown, 500 paragraphs with six distinct inline formulas and one display formula each | 625 / 628 ms | 612 / 20 ms | 13.5 / 16.6 MiB |

The corpus's equations need 179 KiB compressed with 10×20-pixel cells and
438 KiB with 20×40 cells, against 1 and 4 MiB as RGBA; its remaining reload time
is text layout and page recording. The Markdown input now keeps all of its
3,500 formulas, which accounts for its higher RSS. Direct styled output is
byte-identical for both inputs at both cell sizes, with unchanged wall time
(640 / 643 ms and 1,015 / 1,012 ms for the Markdown input) and peak RSS. The
executable grew by 544 bytes.
