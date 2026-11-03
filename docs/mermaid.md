# Mermaid rendering

Mermaid fences use a native Rust backend through the shared image placement path.
No browser, Node.js, separate renderer installation, network request, or persistent
cache is needed. Rendering is optional: `--mermaid auto` follows the existing
Ghostty/Kitty hints, `--mermaid off` keeps code, and `--mermaid kitty` enables a
compatible terminal subject to the usual graphics restrictions. Math and local
image settings remain independent.

`--show-mermaid-source` prints selectable code below successful diagrams.
Unsupported cases, errors, and resource limits keep the complete code panel.
Pipes and `--output text` keep their existing source-based representation;
`--output source` remains exact Markdown. None of these export paths starts a
diagram worker. A nonempty `NO_COLOR` disables text styling but retains selected
graphics; `--light` changes the diagram palette.

## Coverage

The shared document-graphics library uses the published
[`document-graphics-mermaid` 0.1.3](https://crates.io/crates/document-graphics-mermaid/0.1.3)
crate, a maintained fork of `mermaid-rs-renderer`, with CLI and PNG features
disabled. Supported families are:

- Flowcharts (`flowchart` and `graph`), including ordinary subgraphs and labels.
- Sequence, class and state diagrams.
- ER relationships and attribute tables, including types, names, PK/FK/UK keys
  and inline comments. Rows stay intact; an incomplete table retains source.
- Pie charts, mind maps, timelines without sections, user journeys, and quadrant charts.

Embedded Noto Sans fonts add Latin, Greek and Cyrillic labels, including accented
and combining characters. KaTeX fonts preserve existing ASCII diagram appearance.
The backend patch supplies the same embedded font database to text measurement;
no system-font discovery or font cache is used. Missing glyphs (including CJK and
emoji), unsupported families, timeline sections, directives/front matter,
clickable nodes and excessive diagrams retain source. Source exports are unchanged.

This remains a native subset: layout and syntax can differ from Mermaid.js.
The [backend fork](https://github.com/garlab/mermaid-rs-renderer/tree/fork-release)
also preserves all ER key badges and fixes contrast and multiline axis labels in
the added chart families. An optional official-renderer adapter remains a possible
route to broader Mermaid.js compatibility.

## Worker, cache, and limits

The parent starts one child of the same executable on the first eligible cache
miss and reuses it within one document render. The pager retains bounded cache
entries across reloads/reflow, resets the per-render budgets, and releases the
helper after every render. Cancellation kills and reaps in-flight work. The child receives only diagram source,
fitting bounds, and theme over private pipes. A versioned protocol bounds request
lengths and validates response dimensions and RGBA byte counts before allocation.
One I/O thread keeps blocked writes and partial replies within the timeout.

The shared `document-graphics` crate owns parsing, layout, bounded SVG/font
rasterization, wire protocol and worker supervision. mdviewer retains its
invocation budget and fitted-image cache. Only this generated SVG route permits simple arrow markers, with
limits on marker count/size and no recursive paint/marker references. Local SVG
files retain their previous restrictions. Images use the same margins, quote
rails, row reservation, padding, and Kitty transport as other graphics.

Diagrams fit the reading-column width without enlargement, subject to the
one-million-pixel and 8,192-pixel edge caps. Height is independent of the terminal
viewport. Tall images use contiguous, cell-aligned bands of at most
`terminal rows - 2` rows (minimum one), keeping cursor movement on screen. This
preserves native-size labels when the full raster fits the existing limits;
very large diagrams still shrink. The pager can crop, scroll and discard bands
independently. A full picture's padded pixels are charged before any band is
written, so output-budget exhaustion keeps complete source without partial art.
Temporary copies and encoding are limited to one band at a time; small diagrams
keep their existing placement path. More readable pixels can increase transport and
terminal scrollback storage, within the same raster and output budgets.

| Limit | Behavior |
| --- | --- |
| 16 KiB source; 1,024 lines/semicolons; 64 ampersands; lexical nesting depth 32 | Reject before starting a worker or native parser. |
| 512 nodes, 1,024 edges, 16 subgraphs, 512 total subgraph memberships | Check parsed structure before layout. |
| 32 sequence participants, 16 frames/boxes, 64 notes, 128 activations | Bound additional sequence structure. |
| 64 state notes; 1 KiB per node label | Reject excessive annotations. |
| 64 pie slices, 128 mind-map nodes/depth 32, 64 timeline periods/128 events, 128 quadrant points; 32 journey actors with scores 1–5 | Bound additional families before layout; chart numbers must be finite and in range. |
| 32 requests and 5 seconds cumulative request time per document render | Later uncached diagrams retain code; cache hits remain available. |
| 2 seconds per reply after worker startup | Kill and reap a stalled worker, then retain source. |
| 8 MiB cache, at most 32 entries | Account for source and fitted RGBA bytes; evict least recently used entries. |
| Generated SVG: 1 MiB, 16,384 XML nodes, 32,768 logical pixels per edge, 64 million logical pixels of area, bounded depth/text | Validate generated SVG before rasterization; local SVG limits remain tighter. |
| 16 simple arrow markers; at most 8 XML descendants and 2 KiB per marker | Reject recursive marker/paint references. |
| 1,000,000 fitted pixels; 8,192 pixels per edge and reading-column width | Validate returned pixels before storing or displaying them. |
| 64 MiB of cell-padded RGBA placements | Shared with local Markdown images, including cache hits. |

On macOS, the shared worker denies file-data reads, writes, networking and child
processes, and the parent samples its resident memory against a 256 MiB ceiling.
Other platforms do not yet have equivalent sandbox/RSS enforcement. Input,
structure and raster limits bound added work; native parser/layout internals
still allocate temporary storage. Unix
workers also set an eight-second CPU limit as a backstop if the parent disappears
during computation. The total request budget includes startup; the reply timeout cannot bound OS
process creation itself.

Cache identity includes exact source, fitting bounds, and light/dark mode.
Renderer version and bundled fonts are fixed within the invocation. Successful
pixels and ordinary render failures are cached; invalid requests rejected by the
parent do not start a worker. A crash, broken protocol, or timeout disables new
requests for the remainder of the invocation while preserving earlier cache
entries and all later text. Cache hits still incur padding and transport costs.

Normal completion, broken output pipes, and request failure drop the worker,
kill/reap it, close the channels, and join the I/O thread. The native worker starts
no descendants and creates no temporary files. After an abruptly terminated
parent, closed pipes end the worker once its current computation returns; the
Unix CPU limit is a backstop for that computation. There is no background service.

## Validation

Tests in `document-graphics` cover the native subset, fitting, known
incompatibilities, source and structure limits, protocol validation, truncated
replies and worker timeout/reaping. mdviewer tests cover policy and unchanged
exports. Pseudo-terminal checks cover graphics detection, modes
independent of math/images, selectable source, list bullets, quote rails, narrow
windows, light/no-color modes, cache reuse after the request budget, and failures
followed by valid diagrams.
Tall-image checks reassemble decoded bands and compare every padded RGBA byte,
check all-or-nothing output budgeting, and exercise contiguous quote/list
placement, pager scrolling, cropping and resize reflow.

Rendered flowchart, sequence, class, state, and grouping fixtures were inspected;
the initial release kept ER attributes as an explicit fallback. The expanded
coverage described above supersedes that restriction.
These raster/screen-model checks do not establish compatibility with a particular
real terminal version. Use `tests/fixtures/mermaid.md` for Ghostty/Kitty checks.

External comparisons also preserve all 37 plain/source exports and unchanged
text, styles, cursor operations, and graphics across 26 control cases.

## Tall diagrams and routing measurements

The viewer pins published `document-graphics` 0.1.3, which uses published
`document-graphics-mermaid` 0.1.3. Registry builds include the routing optimization
measured below and the quadrant-axis text color fix. The source/graph, raster,
worker and output limits above remain unchanged. Normal builds resolve both
graphics crates from crates.io; optional development overrides belong in ignored
Cargo configuration.

The measurements below were recorded before those releases, comparing the
previous registry versions with a local development build. They retain those
original build labels and results as the performance record.

The patch rejects distant segment pairs before exact intersection/proximity
calculations, preserving endpoint tolerances, and removes temporary node-ID
allocations during path cleanup. Search tiers, candidates, scoring and pass
counts are unchanged. A native probe of the 32-node/60-edge chained flowchart
reduces median layout time from **368.4 to 221.0 ms**, with byte-identical SVG.
All 61 backend benchmark fixtures also retain identical SVGs. The chain retains
19 crossings, 95 bends and zero endpoint/node violations; this improves cost,
not route quality.

Measured on Apple M2 Pro / 16 GiB, macOS 26.4.1, Rust 1.99.0. The following
end-to-end figures use release builds with thin LTO and one codegen unit,
`bench/run.py` harness version 2, a drained 100×40 PTY with 10×20-pixel cells,
80 content columns and two-cell margins. Medians use seven fresh processes after
one warm-up, alternating binary order. The input is generated
`mermaid-complex.md`. Raw samples and images remain outside the repository.

| Build | Wall | CPU | Reported process RSS | Padded pixels | Image bands | Output |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| `26f5b20`, graphics 0.1.1 | 412.5 ms | 409.8 ms | 12.14 MiB | 80×760 | 1 | 23,766 B |
| Width-based sizing, graphics 0.1.2 | 422.5 ms | 419.5 ms | 19.33 MiB | 320×3,060 | 5 | 135,558 B |
| Same sizing, local routing patch | 262.0 ms | 258.9 ms | 19.63 MiB | 320×3,060 | 5 | 135,558 B |

The larger image keeps readable labels and stays below the existing pixel cap.
It costs more raster memory and transport. A separate five-run, 20 ms process-tree
sampling pass observed median peaks of **14.25 MiB before and 28.95 MiB with the
local patch**, with two processes. Sampling can miss short peaks; these are
observations, not resource ceilings. The unsampled `wait4` RSS column is not
aggregate process-tree memory. Emulator decoding, GPU memory, scrolling and
image retention are outside both measurements. Executable sizes stay effectively
unchanged: 16,978,560 bytes before, 16,978,576 with published dependencies, and
16,978,496 with the local patch.

Seven-sample controls with the local patch versus `26f5b20` remain stable:
1 MiB prose **52.6 → 52.4 ms**, 10 MiB mixed Markdown **964.2 → 960.5 ms**,
plain-pipe mixed output **94.4 → 93.5 ms**, the 13-diagram fixture
**46.6 → 46.1 ms**, and 100 repeated diagrams **24.4 → 24.7 ms**.
Control output byte counts are unchanged; process RSS differs by less than
0.3 MiB. Small differences are sensitive to scheduling and PTY draining.

Validation passes with both registry and patched builds: 62 viewer Rust tests,
44 direct-output PTY tests, and 11 pager PTY tests. The locked registry build also
passes the Rust 1.90 minimum-version check. The backend patch passes
541 unit/integration tests with CLI enabled, formatting and Clippy. Shared-library
and GUI-consumer worker checks cover 13 fixture families in both themes. Decoded
raster inspection confirms readable labels across band boundaries; this does
not replace testing scrollback in actual Ghostty and Kitty versions.

Integration with the published 0.1.3 graphics crates passes those viewer checks
without local Cargo overrides. Eighteen terminal/export comparisons match the
validated development build byte for byte, including Mermaid at 24 and 100
columns in both themes, ordinary Markdown, math, tables, local images, and exact
source exports. The release binary uses registry dependencies throughout.

Generate fresh corpora with `bench/generate.py`; it now records the expanded
13-diagram fixture and uses 513 nodes for the oversized-graph fallback. Existing
corpora metadata may reflect older supported subsets. Bands count as individual
Kitty image transmissions, so explicitly declare the expected total per corpus
when comparing this renderer with an older binary:

```sh
python3 bench/run.py --work-dir "$bench_dir" --binary "$renderer" \
  --renderer mdviewer/terminal-mermaid --corpus mermaid-complex.md \
  --samples 7 --warmups 1 --diagram-tiles mermaid-complex.md=5
```

Omit the override for the old single-image build. The runner records overrides
in its metadata and still requires an exact image count, successful exit and
empty diagnostics. Run `--process-tree` separately from timing measurements.
The fixed initial performance baseline remains unchanged.

## Backend comparison

The comparison uses Apple M2 Pro, 16 GiB RAM, macOS 26.4.1, warm filesystem caches,
one warm-up and three fresh measured processes per input. The native SVG probe
uses `mermaid-rs-renderer` 0.3.1, approximate text metrics, and a release build.
The official CLI is 12.0.0 with Puppeteer 25.16.0 and an existing Chrome Headless
Shell 143.0.7499.42. Default SVG font embedding remains enabled in the official
CLI. These are local SVG-generation comparisons, not identical pixels or the
complete terminal rendering pipeline.

| SVG input | Native time | Native process RSS | Official CLI time | Official sampled tree RSS |
| --- | ---: | ---: | ---: | ---: |
| Six-node decision flowchart | 23.36 ms | 6.25 MiB | 660.75 ms | 559.09 MiB |
| Two-participant sequence | 3.07 ms | 3.64 MiB | 456.05 ms | 479.98 MiB |

The official runs sample concurrent Node/browser descendants through numeric
`ps` fields. Sampling adds overhead and can miss short allocation peaks. The
native probe has no descendants, so its process peak is read from `wait4` without
polling. The official library's reusable-browser API was not benchmarked; these
numbers describe fresh CLI invocations. They must not be extrapolated to every
possible browser adapter or diagram type.

The temporary official installation contains about 356.38 MiB of package files;
the existing headless browser adds 184.26 MiB. These are logical on-disk sizes,
excluding package-manager caches and not download sizes. The native dependency
adds about 2.44 MiB to mdviewer's executable and needs no separate installation.
This cost, plus the tested compatibility subset, motivates the initial choice.

## Viewer measurements

The control is `e0239c4`, with local images and the table-border fix. The fixed
[initial baseline](performance-baseline.md) and raw samples remain unchanged.
Both builds use Rust 1.99.0, release optimization, thin LTO, one codegen unit, and
stripped symbols. Measurements use the existing drained 100 by 40 PTY, 10 by
20-pixel cells, and an 80-column reading width. No emulator paints the output.

All 61 cases per build completed with expected image counts and no diagnostics:
488 processes including warm-ups. The previous 30 input hashes are unchanged;
seven Mermaid documents add 14 terminal/pipe cases. Local results and large
generated inputs remain external.

| Unchanged path | Before time | Mermaid build time | Before RSS | Mermaid build RSS |
| --- | ---: | ---: | ---: | ---: |
| Mixed, 10 MiB | 1,154.94 ms | 1,095.95 ms | 177.44 MiB | 177.58 MiB |
| Prose only, 10 MiB | 584.19 ms | 567.05 ms | 167.42 MiB | 167.95 MiB |
| Plain pipe, mixed 10 MiB | 90.74 ms | 95.22 ms | 138.59 MiB | 138.97 MiB |
| Single table, 10 MiB | 699.90 ms | 641.03 ms | 237.19 MiB | 236.41 MiB |
| Graphics, 5,000 equations | 3,509.37 ms | 3,624.24 ms | 15.98 MiB | 16.16 MiB |

One initial timing review trigger, source-TeX rendering of 5,000 equations, did
not reproduce in seven-sample controls: 31.62 versus 31.43 ms, 10.62 versus
10.83 MiB, and first output 6.64 versus 6.68 ms. After that repeat, no unchanged
path crossed the feature design's timing, first-output, or RSS review triggers.
The 10 MiB mixed case remains well below the initial roughly 332 MiB baseline.

| Mermaid input | Enabled time | First output | Reported process RSS | Output | Diagrams |
| --- | ---: | ---: | ---: | ---: | ---: |
| Six-diagram fixture plus fallbacks | 64.80 ms | 31.71 ms | 10.19 MiB | 2.96 MiB | 6 |
| Three-node flowchart | 8.95 ms | 7.46 ms | 7.92 MiB | 0.17 MiB | 1 |
| Two-participant sequence | 11.67 ms | 7.82 ms | 7.41 MiB | 0.64 MiB | 1 |
| 100 identical flowcharts | 98.06 ms | 8.73 ms | 7.81 MiB | 17.16 MiB | 100 |
| 40 distinct flowcharts | 42.29 ms | 7.94 ms | 8.48 MiB | 3.75 MiB | 32 |
| 32-node, 60-edge graph | 442.49 ms | 438.43 ms | 10.22 MiB | 0.31 MiB | 1 |
| Oversized/invalid/unsupported, then valid | 13.64 ms | 5.45 ms | 7.92 MiB | 0.27 MiB | 1 |

Reported process RSS comes from `wait4` and is not the concurrent worker-tree
total. Separate sampling observed one viewer plus one worker: 16.77 MiB for the
six-diagram fixture, 11.89 MiB for cached repeats, 14.14 MiB for distinct diagrams,
and 10.42 MiB for the complex graph. These sampled values can miss transient
peaks; short single-diagram runs are especially easy to undersample. Timing in
the table excludes this sampling overhead.

The complex graph spends most of its time in native layout; a two-second deadline
keeps worse cases from monopolizing the viewer. The 40-distinct input renders 32
diagrams and retains all remaining source. Cached repeats avoid layout work but
still transmit pixels. The measurements above predate compressed transport; see
[streaming and graphics measurements](streaming-and-graphics.md) for its effect.
Terminal-side image reuse remains follow-up work.

Nine disabled-feature cases add 36 processes. The small/medium diagram inputs
take 3.27–4.32 ms with about 3.06–3.27 MiB RSS; the large fallback document takes
7.53 ms and 3.72 MiB. The two large prose/mixed controls retain their normal
profiles. All seven Mermaid pipe cases take roughly 3.3–3.7 ms and emit no images.
The worker never starts on disabled, plain/source, or unsupported-terminal paths.

The release executable grows from 13,140,384 to 15,696,816 bytes
(12.53 to 14.97 MiB, about 19.5%). Its SHA-256 is
`6cb6ac6d17072b8fa7ff9a70f0cb94ba653fcb227d4f0e64c9a24b3c8e8380b7`.
Seven lockfile packages are added: `mermaid-rs-renderer`, `anyhow`, `json5`,
`once_cell`, `ucd-trie`, `memmap2`, and `fontconfig-parser`. Existing dependency
features expand, but no new font assets or private checkout are required.

Generate inputs using the [benchmark instructions](performance-baseline.md).
Select `--corpus 'mermaid-*' --renderer mdviewer/terminal-mermaid`; compare
`--mermaid-mode auto` and `--mermaid-mode off`. Use `--mermaid-mode legacy` for
older binaries without the flag. Run `--process-tree` separately for approximate
concurrent RSS; it requires permission to inspect process relationships.
PTY measurements exclude emulator decoding, GPU work, and scrollback memory.

## Shared library

`document-graphics` is a separate library repository containing the renderer, restricted SVG
rasterization, bundled-font setup and worker supervision. The CLI retains its
five-second invocation budget, 32-attempt limit and 8 MiB cache. GUI consumers can
choose different scheduling and cache policies without duplicating renderer code.
Math uses the published `katex-native` 0.1.0 crate.

Extraction validation compared release binaries with published math dependencies
on both sides, using Rust 1.99 and macOS on Apple Silicon. All 16 text/source
exports and 32 terminal cases were byte-identical, including image payloads;
12 GUI diagram PNGs also matched exactly. Rust 1.90 checks cover the complete
workspace and all targets. The shared crate's packaged sources build independently.

Five fresh measured processes per case, after one warm-up and with before/after
order alternated, gave these median wall times. Builds finished before measuring.
Terminal cases include PTY I/O and exclude emulator painting:

| Case | Before | Shared library |
| --- | ---: | ---: |
| Plain export, 312 KB prose | 8.75 ms | 8.72 ms |
| Terminal text, same prose | 34.28 ms | 34.30 ms |
| Mermaid fixture | 49.17 ms | 54.58 ms |

The terminal worker now also installs the macOS sandbox and resident-memory guard
already used by the GUI. The Mermaid case includes that additional startup work;
these samples do not isolate its cost or certify a latency percentile. Existing
performance baselines remain unchanged; generated outputs and measurements stay
local.

## Extraction review

A second release-build comparison against the accepted pre-extraction build
(`ba45167`) used the same external corpora, 100×40 PTY, 10×20 pixel cells, warm
filesystem caches, one warm-up and five alternating fresh processes per build.
Medians on Apple Silicon macOS:

| Case | Before | Shared library | Reported peak RSS before / after |
| --- | ---: | ---: | ---: |
| 10 MiB mixed, styled terminal | 1.129 s | 1.108 s | 58.23 / 58.22 MiB |
| 1 MiB prose, styled terminal | 68.89 ms | 67.70 ms | 9.47 / 9.45 MiB |
| 100 equations | 43.46 ms | 43.78 ms | 10.08 / 10.05 MiB |
| 10 MiB mixed, plain export | 96.17 ms | 95.80 ms | 54.72 / 54.73 MiB |
| SVG with text | 3.86 ms | 4.00 ms | 5.53 / 5.58 MiB |
| Six Mermaid diagrams | 49.93 ms | 54.62 ms | 9.98 / 11.33 MiB |
| 100 repeated Mermaid placements | 19.64 ms | 24.38 ms | 8.02 / 10.19 MiB |

The worker restrictions add about 5 ms of fixed startup cost on this machine.
Unchanged paths remain within run-to-run variation. RSS is the benchmark
harness's `wait4` process statistic, not a simultaneous sum of parent and worker
memory; PTY measurements exclude terminal painting and scrollback storage.
All 32 fixture/theme/graphics combinations and 82 text/source exports were
byte-identical. The 42 PTY checks, CLI tests, shared worker tests and GUI consumer
integration passed. The review also fixed viewport bounds above the library's
raster limit and rejection of worker replies arriving after their deadline.
Large corpora and raw results remain outside the repository.

## Expanded native coverage: resource check

The Unicode/ER/chart changes were compared with the reviewed extraction build
using release binaries, one warm-up, five alternating fresh processes per build,
and the same PTY/corpora methodology above. These are paired local observations;
absolute times from separate benchmark sessions are not directly comparable.

| Unchanged case | Before | Expanded backend | Reported RSS before / after |
| --- | ---: | ---: | ---: |
| 10 MiB mixed, styled terminal | 1.248 s | 1.203 s | 58.00 / 58.28 MiB |
| 1 MiB prose | 68.64 ms | 67.79 ms | 9.45 / 9.39 MiB |
| 100 equations | 45.09 ms | 45.34 ms | 10.36 / 10.31 MiB |
| 10 MiB plain export | 97.78 ms | 97.59 ms | 54.72 / 54.62 MiB |
| ASCII SVG with text | 4.24 ms | 4.19 ms | 5.56 / 5.56 MiB |
| One flowchart | 13.09 ms | 13.13 ms | 10.22 / 10.30 MiB |
| One sequence diagram | 13.06 ms | 12.91 ms | 9.17 / 9.36 MiB |
| 100 repeated flowchart placements | 24.96 ms | 25.79 ms | 10.23 / 10.34 MiB |

The release binary grows from **15.10 to 15.79 MiB** (+0.69 MiB, 4.6%), mostly
the two bundled Noto Sans faces. Existing paths show no material latency or RSS
regression; repeated placements remain cached. The older mixed Mermaid fixture
now renders eight diagrams instead of six, so it is not an equal-work latency
comparison. Input, worker, pixel, cache and output budgets are unchanged.

All twelve original diagram/theme RGBA results are byte-identical. Expanded
coverage is checked through 26 shared-worker renders, the corresponding GUI
consumer integration, and 43 CLI PTY tests including narrow/light layouts and
exact source exports. Visual review covers both palettes and complete ER columns.
Formatting, Clippy for maintained code, Rust 1.90, and both package archives are
checked. The backend's inherited lint warnings are excluded with `--no-deps`;
its source delta and license are recorded in document-graphics.
