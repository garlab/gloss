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
space and cursor rewinds. It does not retain every raster or the full ANSI output
in RAM. Temporary files are removed automatically when their descriptors close.

Only visible rows are replayed. Kitty image numbers identify uploads owned by
this view; adjacent scroll positions reuse visible image data. Cropping happens
in source pixels so partial equations and diagrams retain their scale. Leaving
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
pending request. Publication swaps the whole page. The previous good page remains
available if a watch read or render fails. Resize reflow temporarily clears an
incompatible viewport rather than replaying rows at the wrong width.

Watch polls the named path's device, inode, size, and modification/change times
every 250 ms. A change must remain stable for a subsequent poll before loading,
so ordinary edits appear after roughly 250–500 ms plus rendering time. This
detects atomic replacement and disappearance/recreation without directory-wide
subscriptions. Unchanged bytes skip rendering. There is no content hashing,
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
`Interrupted` indefinitely. Cleanup restores the terminal before waiting for the
active renderer and its bounded diagram worker to finish.

## Resource limits

| Resource | Limit per page/render |
| --- | --- |
| UTF-8 Markdown input | 64 MiB |
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

The existing math, image, code, table, and Mermaid budgets apply too. Full source,
syntax projections, parser storage, and large individual blocks still scale with
input. Keeping a displayed page while building a replacement can temporarily use
two page spools. These are component limits, not an overall RSS guarantee.

Pager storage exhaustion reports an error instead of truncating content; watch
retains the last good page. Direct output and exports remain alternatives for
larger files, with their existing resource policies. The pager initializes no
threads, signal handlers, temporary files, or polling on those paths.

## Validation

Run `python3 tests/pager_smoke.py target/debug/mdviewer` after building. Its
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
python3 bench/pager.py --binary target/release/mdviewer \
  --input "$TMPDIR/mdviewer-bench/corpora/mixed-10m.md"
python3 bench/pager.py --binary target/release/mdviewer \
  --input "$TMPDIR/mdviewer-bench/corpora/mermaid-repeated-100.md" \
  --watch-seconds 3
```

The script reports readiness, individual navigation latencies, quit time,
process-reported RSS, CPU, output bytes, and idle helper count as JSON. Use
`bench/run.py` for direct-output comparisons and separate process-tree sampling.
