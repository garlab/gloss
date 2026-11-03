# Streaming layout and compressed graphics

This pass adds Markdown compatibility while reducing retained layout and graphics
transport costs. The fixed [Glow comparison baseline](performance-baseline.md)
and its raw samples remain unchanged. Large inputs, raw local results, and
comparison scripts remain external to the repository.

## Implementation

- The CLI visits completed blocks, with one-block lookahead for quote rails.
  Plain export writes directly and streams table rows. A retained-layout API
  remains available. Source, parser storage, formatted tables, code blocks, and
  long logical lines can still scale with input.
- Optional source spans use a compact two-word representation. This accommodates
  hyperlink targets without enlarging every retained table/code run.
- Math uses a lazy 8 MiB, 64-entry LRU raster cache. Source keys are at most 16 KiB;
  accounting includes pixels, TeX, and entry overhead. Keys include rendering
  mode, font size, color, bounds, and equation-numbering state. Cached pixels are
  shared; display numbering has the same behavior on hits and misses.
- Kitty transport uses fast zlib compression when it saves at least 16 bytes.
  Base64 encodes 3,072-byte input chunks into a fixed 4,096-byte stack buffer.
  Decoded pixels, placement, source fallbacks, and terminal-allocated image IDs
  are unchanged. Incompressible data uses raw RGBA.
- Hyperlinks, alerts, inert front matter, expanded details, and backslash math
  have explicit policies and limits in [Markdown extensions](markdown-extensions.md).
  [Terminal discovery](terminal-detection.md) remains input-free.

Compression uses the already locked `flate2` dependency directly; no package or
helper process is added. The existing local-image/Mermaid output budget still
counts padded RGBA bytes, including cache hits, regardless of compression.

## Comparison method

Control: `7280ec5`. Both builds use release optimization, thin LTO, one codegen
unit, stripped symbols, and the same local math dependency. Measurements use
Apple M2 Pro, 16 GiB RAM, macOS 26.4.1, and Rust 1.99.0, with warm filesystem
caches, one warm-up, and three fresh measured viewer invocations per case.
The drained PTY is 100 by 40 cells with 10 by 20 pixel cells; narrow cases use
14 columns. Content width is 80 with two-column margins.

The final comparison has 61 control cases and 71 updated cases: 528 viewer
invocations including warm-ups. All finished successfully with expected image
counts and no diagnostics. Ten added cases cover extension rendering, plain
exports, repeated backslash math, and exhausted extension budgets.

Control executable SHA-256:
`6cb6ac6d17072b8fa7ff9a70f0cb94ba653fcb227d4f0e64c9a24b3c8e8380b7`.
Updated executable SHA-256:
`b17a1a11acc317a131dc65b449ceab3b37b6ce6cd1842c49b4318aba88619cf7`.

| Input | Before time | Updated time | Before peak RSS | Updated peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Mixed 10 MiB | 1103.76 ms | 1075.93 ms | 177.70 MiB | 58.19 MiB |
| Prose 10 MiB | 595.92 ms | 601.35 ms | 167.47 MiB | 50.45 MiB |
| Formatted table 10 MiB | 715.37 ms | 703.97 ms | 237.36 MiB | 237.45 MiB |
| Plain mixed 10 MiB | 92.02 ms | 93.49 ms | 138.92 MiB | 54.72 MiB |
| Plain table 10 MiB | 124.18 ms | 118.65 ms | 319.59 MiB | 101.41 MiB |
| 5,000 equations, graphics | 3592.09 ms | 2007.19 ms | 16.06 MiB | 15.41 MiB |
| 1,000 repeated equations, graphics | 592.00 ms | 52.00 ms | 9.53 MiB | 8.84 MiB |
| 5,000 equations, source TeX | 31.17 ms | 31.21 ms | 10.75 MiB | 7.97 MiB |

The mixed document's peak RSS falls by about 67%, with similar rendering time.
The original pre-highlighter Glow comparison was approximately 1.16 seconds and
332 MiB for the same mixed input; the new result remains below both values.
Formatted tables still retain a whole table, so that worst case remains a memory
optimization target. Their memory is effectively unchanged in this pass.

Mixed first output falls from 86.17 to 36.61 ms; plain-table first output falls
from 116.01 to 27.08 ms. First output means the first observed byte, not completed
screen painting. Neither streaming nor the parser is a constant-memory reader.

An exploratory source-TeX timing increase did not persist. A separate seven-sample
repeat measured 30.28 versus 30.66 ms and 10.73 versus 7.94 MiB for 5,000 equations,
with identical output bytes. No unchanged case in the final comparison or repeat
crossed the feature design's timing, first-output, or RSS review triggers.

## Output and binary costs

| Input | Before output | Updated output |
| --- | ---: | ---: |
| Mixed 10 MiB | 26.68 MiB | 27.72 MiB |
| Prose 10 MiB | 14.81 MiB | 16.78 MiB |
| 5,000 equations, graphics | 183.56 MiB | 14.22 MiB |
| 1,000 repeated equations, graphics | 30.41 MiB | 2.26 MiB |
| Mermaid fixture, six diagrams | 2.96 MiB | 0.09 MiB |

Graphics output falls by roughly 92% for the 5,000-equation input. Repeated
formulas also avoid layout/raster work, giving about an eleven-fold speedup on
the repeated-equation case. Each placement still transmits pixels; terminal-side
image reuse is not implemented.

Text output grows because recognized terminals now receive clickable targets.
With `--links off`, mixed and prose output bytes exactly match the control:
26.68 and 14.81 MiB. Separate disabled-link runs measured 1,081.04 and 583.86 ms,
with 58.19 and 50.44 MiB peak RSS respectively. Small timing differences between
samples should not be treated as guaranteed improvements.

The executable grows from 15,696,816 to 15,815,040 bytes
(14.97 to 15.08 MiB), about 0.75%. No new lockfile packages are required.

## Extension costs

| Input and mode | Time | Peak RSS | Output |
| --- | ---: | ---: | ---: |
| Small extension fixture, graphics | 15.86 ms | 10.98 MiB | 0.005 MiB |
| 1,000 repeated backslash equations, graphics | 21.85 ms | 7.12 MiB | 0.665 MiB |
| Repeated details, terminal | 105.87 ms | 24.20 MiB | 2.741 MiB |
| Repeated details, plain | 31.61 ms | 23.94 MiB | 0.808 MiB |
| Oversized extensions, terminal | 17.02 ms | 5.80 MiB | 0.402 MiB |

The repeated-details input deliberately exhausts the wrapper-edit budget; later
wrappers remain literal while their content survives. The oversized input checks
front matter, details, and delimiter limits, plus code exclusions. Extension
candidates can require preliminary parsing and a source projection; absent
features bypass that work. Pipes skip graphics, highlighting, and hyperlinks;
exact-source output bypasses parsing entirely.

## Validation and reproduction

All 57 Rust tests and 42 PTY tests pass, alongside formatting and Clippy checks.
Checks cover original ranges after projections, Unicode and adjacent delimiters,
code/link exclusions, nested and unclosed details, numbered cached equations,
compression round trips and chunk limits, hyperlink scoping, geometry/theme
precedence, and preserving pending stdin during file rendering.

An external comparison verified resolved text/attributes and cursor operations
for 26 existing terminal cases. Compressed graphics were decoded before comparing
pixel hashes and dimensions. The 36 unchanged plain exports match byte for byte;
the CommonMark specimen intentionally retains its front matter as literal
metadata, with its body unchanged. All 37 original corpus source exports remain
byte-exact. New extension source preservation is covered separately by tests.

PTY timing and process RSS exclude terminal decoding, GPU work, scrollback memory,
and native selection. Compression reduces transport volume, not the number of
image placements or their decoded pixel storage. Live Ghostty/Kitty checks remain
outstanding; the PTY suite does not establish visual compatibility across fonts,
zoom levels, or terminal versions.

Generate the external corpus with the [baseline procedure](performance-baseline.md).
The generator now also creates the four `extensions-*.md` inputs. For the updated
viewer, run all mdviewer cases with three samples and one warm-up; omit the Glow
renderer when comparing these two viewer builds. Use `--corpus 'extensions-*'` to
isolate the added cases and `--no-links` for a hyperlink-disabled control.
Preserve each local `results.json` before running another comparison. Do not
commit generated corpora, raw results, or captured terminal output.

## Native-pixel equation transport

Equations now retain their original raster dimensions during transport. Layout
still reserves the same cell rows and columns; the writer moves to the starting
cell and uses a within-cell vertical offset instead of allocating a transparent
cell-sized copy. It omits Kitty's target `c`/`r` rectangle to preserve native
pixel size. The pager records offsets and clips in source pixels, including
partial first/last rows. Ordinary image and diagram bands keep their existing
transport and output accounting. See the
[Kitty placement protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/#controlling-displayed-image-layout).

On the same hardware and release profile described in [pager measurements](pager.md#reload-and-reflow-performance),
one warm-up and five alternating fresh processes compared `885be54` with this
change using the generated 1,000-equation corpus:

| Measurement | Before | After |
| --- | ---: | ---: |
| Transmitted decoded pixels | 6,691,600 | 4,657,521 |
| PTY output bytes | 2,810,510 | 2,702,517 |
| Median wall time | 393 ms | 389 ms |
| Median CPU time | 377 ms | 374 ms |
| Median peak viewer RSS | 11.92 MiB | 11.67 MiB |

This removes 30.4% of decoded pixels and 3.8% of transmitted bytes. Elapsed time
is effectively unchanged: the original equation is already rasterized before
cell padding is added, and transparent pixels compress cheaply. Terminal GPU
cost and scrollback memory were not measured.

Decoded equation ink, absolute pixel coordinates and text positions matched the
previous build across 24 fixture/width/cell-size/theme combinations. Pager tests
cover native offsets and clipping at both viewport edges. Live Ghostty/Kitty
visual validation remains separate from these protocol/pixel checks.
