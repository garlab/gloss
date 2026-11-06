# Table rendering

Pipe tables retain rows, cells, column alignment, inline runs, and source spans.
Terminal output uses accent-colored bold headers, muted separators, and wrapped
cells. Left/default, center, and right alignment apply to each physical cell
line. The table fits the same reading column and margins as surrounding text.

Inline emphasis, code, links, escaped pipes, Unicode graphemes, and equations
use the existing rendering rules. Images remain indivisible; cells in a physical
row share the inline math baseline and reserve enough terminal rows. Quote rails
and list indentation continue through the table. `--show-math-source` prints
selectable TeX after the physical row containing each rendered equation.

Vertical separators and quote rails span every reserved row above and below the
text baseline. This fixes gaps beside tall fractions and roots. List bullets are
printed once. Pseudo-terminal checks cover border continuity with color disabled,
neighboring cells, and a short viewport. The measurements below describe the
original table implementation; [local images](images.md) records subsequent
measurements including this border correction and shared raster placement.

`NO_COLOR` removes styling while retaining table structure and the selected math
mode. An unsupported terminal or `--math tex` retains source TeX in cells. Raw
HTML remains text; HTML table layout, merged cells, and HTML line-break handling
are not implemented.

## Work and storage limits

| Limit | Behavior |
| --- | --- |
| 16 columns in a grid | Wider tables use numbered cells. |
| At least 20 content cells for a multi-column grid | Narrower tables use numbered cells. |
| At least 3 cells per column, plus 3 per separator | Fall back when even these widths cannot fit. |
| First 64 rows and 64 KiB of decoded text per table | Bound width estimation; later content wraps to the chosen widths. |
| 64 KiB of decoded text per cell | A larger cell switches its row to numbered cells. |
| 256 KiB of decoded text, 4,096 inline runs, and 32 formulas per row | An over-limit row uses numbered cells. |
| 8 MiB of retained unpadded equation pixels per grid row | Reduce fitting height to the remaining budget; preserve TeX when no image can fit. |

Width sampling never truncates output. All rows and cells are rendered, including
content beyond the sample and cells beyond the column limit. Fallback shows
`1:`, `2:`, and subsequent column numbers, with blank lines between rows. Header
cells keep their header style. Extremely narrow windows shorten the labels to
leave room for content. Fallback equations always remain selectable source TeX,
avoiding a large collection of rasters for an over-limit row.

The grid retains wrapping and rasters for only one row at a time. Fallback uses
the ordinary logical-line renderer, without collecting a row of wrapped cells.
These limits bound added table decoration work, not whole-process memory: the
original input, parser working storage, complete decoded layout, and a long
logical line still scale with input. Native math can use temporary working memory
before producing a fitted raster; image padding and transport also allocate
separately from the retained-pixel budget.

## Exports and dependencies

Plain pipes and `--output text` retain the previous flat representation:
decoded cell contents followed by ` | `, with newlines between rows. They do not
add alignment padding, borders, numbered-cell fallbacks, width sampling, or math
images. Exact source export remains byte-preserving, including CRLF and the final
newline state. Neither representation is recovered from terminal output.

No dependency, external helper, grammar, or math-library change is required.

## Validation

Tests cover alignment, cell and table source ranges, styled/escaped content,
missing cells, Unicode wrapping, sampling boundaries, wide/narrow/oversized
fallbacks, unchanged exports, nested lists/quotes, dark/light/no-color modes,
invalid formulas, selectable TeX, and graphics placement across neighboring
cells. The screen model checks cursor operations and image rectangles; it does
not establish visual compatibility with a particular emulator or font.

Use `tests/fixtures/tables.md` at normal and narrow widths for manual checks.
Large table inputs and generated results remain outside the repository. The
benchmark generator adds large single tables, excessive columns, over-limit
cells/rows, and a 100-equation table. Select `--corpus 'tables-*'` with the runner;
the `gloss/terminal-narrow` case uses 14 columns to exercise fallback.

## Performance comparison

The control is `1d47620`, with syntax highlighting and the previous rendering
optimizations. The [initial baseline](performance-baseline.md) and its raw
artifact remain fixed. These are same-machine release measurements using the
baseline method: Apple M2 Pro, 16 GiB RAM, macOS 26.4.1, Rust 1.99.0, thin LTO,
one codegen unit, stripped symbols, warm filesystem caches, one warm-up, and
three fresh measured processes per case. Values below are medians. Normal
terminal runs use a drained 100 by 40 PTY and source TeX unless marked as graphics;
the narrow case uses 14 by 40. No emulator paints the output.

The final comparison completed 29 cases per build, or 232 successful processes
including warm-ups, without diagnostics and with expected image counts. The
existing 16 input hashes are unchanged; five table inputs add nine cases.

| Input | Before time | Tables time | Before peak RSS | Tables peak RSS |
| --- | ---: | ---: | ---: | ---: |
| CommonMark, 200 KiB | 17.73 ms | 17.88 ms | 5.81 MiB | 5.88 MiB |
| Mixed, 1 MiB | 119.47 ms | 122.88 ms | 24.03 MiB | 25.27 MiB |
| Mixed, 10 MiB | 1,032.29 ms | 1,086.00 ms | 162.59 MiB | 177.33 MiB |
| Prose only, 10 MiB | 574.79 ms | 595.04 ms | 167.16 MiB | 167.19 MiB |
| Plain pipe, mixed 10 MiB | 92.37 ms | 89.45 ms | 138.66 MiB | 138.61 MiB |
| Single table, 1 MiB | 63.98 ms | 74.42 ms | 27.95 MiB | 27.52 MiB |
| Single table, 10 MiB | 616.74 ms | 707.68 ms | 329.50 MiB | 237.08 MiB |
| Plain pipe, table 10 MiB | 120.91 ms | 120.97 ms | 319.27 MiB | 319.41 MiB |
| 14-column fallback, table 1 MiB | 74.27 ms | 124.17 ms | 27.91 MiB | 27.58 MiB |
| 128-column table | 4.41 ms | 6.34 ms | 3.81 MiB | 3.50 MiB |
| Oversized rows/cells | 19.96 ms | 22.22 ms | 5.06 MiB | 4.05 MiB |
| Table graphics, 100 equations | 18.75 ms | 20.76 ms | 6.80 MiB | 6.86 MiB |

The mixed 10 MiB document contains thousands of small tables: time rises by about
5% and RSS by 9% as it gains aligned cells. Terminal output grows from 23.71 to
26.68 MiB. Ordinary prose and plain exports retain their previous memory profile.
The single 10 MiB table uses about 28% less peak RSS, while wall time rises by
15%; output grows from 15.67 to 20.84 MiB. Numbered-cell fallback also adds labels
and lines: the 1 MiB narrow case emits 4.04 MiB instead of 2.42 MiB. These are
feature-enabled presentation costs, separate from unchanged paths.

The first observed byte for mixed 10 MiB changes from 81.76 to
84.77 ms; the single 10 MiB table changes from 82.85 to
99.06 ms. This includes building the layout and can precede useful content.

Cell runs use compact owned slices, and row vectors release spare capacity.
Width estimates leave two cells of growth allowance when sampling stops early,
so later identifiers and numbers need not wrap immediately beyond the sample.
Neither choice changes source content. Full source/layout storage remains the
main memory limit on very large inputs.

Two initial graphics timing review triggers did not reproduce in seven-sample
controls (one warm-up each, 32 additional processes):

| Unchanged graphics input | Before time | Tables time |
| --- | ---: | ---: |
| 5,000 equations | 3,504.63 ms | 3,510.93 ms |
| 1,000 repeated equations | 573.54 ms | 581.51 ms |

After these repeats, no unchanged path crossed the feature design's timing,
first-output, or RSS review triggers. These small local samples are reference
measurements, not portable performance guarantees. PTY timing excludes emulator
image decoding, GPU work, and scrollback; renderer RSS excludes emulator memory.

The release executable grows by 16,624 bytes, from 12,653,792 to 12,670,416 bytes
(12.07 to 12.08 MiB). Its SHA-256 is
`eadb9c6625eefed4414c327130e08e7b9a21fb24a769b9419b9fbaf0778517ce`.
The lockfile and dependency set are unchanged.

All 35 Rust tests and 28 pseudo-terminal tests pass, together with formatting and
Clippy checks. External before/after checks also preserve all 21 plain exports
and exact-source exports, and text/styles/cursor operations/image payloads on
16 cases without changed table presentation. Large inputs, raw local results,
and generated output remain external.
