# Rendering performance improvements

The first syntax-highlighting implementation increased work and executable size.
These optimizations retain its language set, query limits, themes, and output
contracts while reducing allocation and terminal output costs. The 10 MiB mixed
input falls from 1.65 to 1.06 seconds and from 253 to 163 MiB peak RSS in a fresh
same-session comparison, with highlighting enabled in both builds.

## Changes

- Parse borrowed events into layout without keeping a second, owned event vector
  and its copied text for the lifetime of the document.
- Wrap borrowed word fragments, allocating text for physical lines instead of
  allocating every grapheme. Preserve Unicode graphemes and indivisible images.
- Generate logical lines with one-line lookahead rather than collecting all of
  them. Move line text into wrapping and sanitize it in place.
- Encode ANSI attributes in a stack buffer and emit only changes within each
  physical line. Reset before graphics and at line boundaries; write padding
  without temporary strings.
- Strip symbols from release executables. Keep the existing optimization level,
  thin LTO, and single codegen unit.

No grammar, feature, highlighter budget, or dependency was removed. Plain output
and exact source output retain their original bytes. Terminal escape sequences
change, while displayed text, resolved colors/attributes, cursor operations, and
image payloads remain equivalent on the checked inputs.

## Measurements

The [baseline method](performance-baseline.md) is unchanged: Apple M2 Pro,
16 GiB RAM, macOS 26.4.1, Rust 1.99.0, release builds, warm filesystem caches,
one warm-up followed by three fresh processes per case. The terminal is a drained
100 by 40 PTY with 10 by 20 pixel cells, 80 content columns, and two-cell margins.
Terminal cases use source TeX unless marked as graphics. Values are medians;
MiB means 1,048,576 bytes.

The before build is the [initial highlighter](syntax-highlighting.md), SHA-256
`aaaa46048f10087872c6e5291f77e9bf246baa6ce5513490d1dbb321c3d9a28a`.
The optimized executable has SHA-256
`d17a316d64a113b45a67511ef5055961282229d7ecbddbac8d751a65ed76d937`.
The math dependency remains at `982f130`. Both builds completed all 20 cases:
160 successful processes including warm-ups, with no diagnostics and the
expected image counts. No case crossed the feature design's timing, first-output,
or RSS regression review triggers against the before build.

| Input | Before time | Optimized time | Before peak RSS | Optimized peak RSS |
| --- | ---: | ---: | ---: | ---: |
| CommonMark, 200 KiB | 31.32 ms | 18.65 ms | 7.88 MiB | 5.91 MiB |
| Mixed, 257 KiB | 58.04 ms | 42.42 ms | 14.56 MiB | 11.98 MiB |
| Mixed, 1 MiB | 185.93 ms | 120.73 ms | 32.89 MiB | 24.14 MiB |
| Mixed, 5 MiB | 840.07 ms | 528.53 ms | 130.28 MiB | 86.28 MiB |
| Mixed, 10 MiB | 1,649.70 ms | 1,056.77 ms | 253.39 MiB | 162.61 MiB |
| Long lines, 0.98 MiB | 82.27 ms | 48.14 ms | 43.19 MiB | 8.12 MiB |
| Math graphics, 100 equations | 71.38 ms | 70.43 ms | 8.05 MiB | 7.86 MiB |
| Math graphics, 1,000 equations | 690.01 ms | 528.21 ms | 10.11 MiB | 9.19 MiB |
| Math graphics, 5,000 equations | 3,671.47 ms | 3,623.52 ms | 19.41 MiB | 15.70 MiB |
| Math graphics, 1,000 repeated values | 596.81 ms | 589.33 ms | 9.58 MiB | 8.89 MiB |
| Plain pipe, mixed 1 MiB | 18.37 ms | 11.96 ms | 26.72 MiB | 17.23 MiB |
| Plain pipe, mixed 10 MiB | 148.54 ms | 91.92 ms | 230.86 MiB | 138.94 MiB |
| Plain pipe, 5,000 equations | 10.08 ms | 7.32 ms | 13.69 MiB | 10.11 MiB |
| Terminal TeX, 5,000 equations | 54.64 ms | 31.05 ms | 13.98 MiB | 10.41 MiB |
| Prose only, 1 MiB | 112.15 ms | 62.01 ms | 31.88 MiB | 16.80 MiB |
| Prose only, 10 MiB | 1,077.27 ms | 597.68 ms | 325.88 MiB | 167.16 MiB |
| Language sample | 75.28 ms | 64.17 ms | 21.38 MiB | 19.92 MiB |
| Multiline comment | 38.61 ms | 26.32 ms | 9.64 MiB | 7.67 MiB |
| Oversized fence | 67.74 ms | 23.02 ms | 8.12 MiB | 3.20 MiB |
| Capture-heavy JSON | 25.80 ms | 22.88 ms | 13.41 MiB | 9.61 MiB |

| Input | Before first output | Optimized first output | Before output | Optimized output |
| --- | ---: | ---: | ---: | ---: |
| Mixed, 10 MiB | 96.19 ms | 84.37 ms | 32.66 MiB | 23.71 MiB |
| Math graphics, 5,000 equations | 7.85 ms | 7.51 ms | 183.62 MiB | 183.56 MiB |
| Plain pipe, mixed 10 MiB | 133.42 ms | 82.15 ms | 9.47 MiB | 9.47 MiB |
| Prose only, 10 MiB | 100.60 ms | 58.79 ms | 17.38 MiB | 14.81 MiB |

For the mixed 10 MiB case, wall time and peak RSS each fall by about 36%; encoded
output falls by 27%. Prose-only peak RSS falls by 49%. The original pre-highlighter
reference remains unchanged in the baseline document and raw artifact; these
new figures use a freshly measured highlighter build as their direct control.

The math rasterization and image protocol are unchanged. Graphics output still
dominates math-heavy workloads; these changes do not solve that cost. Three
samples are a local reference, not a statistical guarantee, particularly for
small differences or the variable graphics timings. PTY timings exclude emulator
painting, image decoding, GPU work, scrollback storage, and emulator memory.
First output means the first observed byte, which can precede useful content.

## Allocation profile

A separate temporary Rust allocator counter, rendering the mixed 10 MiB input
into a buffered sink, found the following. Counts include Rust allocation and
reallocation calls, exclude Tree-sitter's C allocations, and are not process RSS.
Instrumentation and the sink differ from the uninstrumented CLI measurements
above; phase timings are deliberately not compared here.

| Rendering phase | Before allocation calls | Optimized allocation calls | Before cumulative allocated bytes | Optimized cumulative allocated bytes |
| --- | ---: | ---: | ---: | ---: |
| Code colors disabled | 20,144,771 | 3,292,797 | 1,235.5 MiB | 291.2 MiB |
| Code colors enabled | 23,957,139 | 4,843,399 | 1,606.5 MiB | 622.0 MiB |

Live Rust allocations after layout fall from 204.5 to 101.9 MiB. Parsing now
happens during layout, so separate parse/layout timing comparisons would be
misleading. The full source and layout still remain resident, and parser working
storage and long logical lines can still grow with input. A constant-memory
reader would require further changes to those boundaries.

## Executable footprint

| Release executable | Bytes | MiB |
| --- | ---: | ---: |
| Initial highlighter | 13,455,248 | 12.83 |
| Optimized | 12,653,792 | 12.07 |

Stripping symbols accounts for most of the reduction. Object-file inspection
found roughly 6 MiB of grammar table data, so removing allocation overhead alone
cannot return the executable to the original 4.93 MiB. Larger reductions need a
separate language-pack or parser-engine decision, with coverage and runtime costs
measured explicitly.

A size-oriented release profile was also tested on an intermediate build. It
saved another 0.41 MiB but slowed the mixed/prose 10 MiB cases and the
1,000-equation graphics control by about 12%. The default speed-oriented profile
was retained. No alternative syntax engine was benchmarked in this work.

## Validation and reproduction

The 30 Rust tests and 24 pseudo-terminal tests pass, as do formatting and Clippy
checks. Coverage includes grapheme wrapping across style boundaries, repeated
event iteration with source ranges, control filtering, and clearing text
attributes between spans and after output.

An external comparison also checked resolved text and ANSI attributes, cursor
operations, and image command payloads against the before binary for all 16
benchmark inputs, four light-theme fixtures, and a graphics case. PTY output
translation was disabled for that byte-level comparison to separate writer output
from PTY newline processing. Plain exports matched the before binary for all
16 inputs; exact source exports matched their inputs byte for byte. These checks
do not replace visual validation in Ghostty and Kitty.

Use the baseline's generator and runner with separate external release binaries
and retain each result before running the next build. Select these renderers to
repeat the 20-case comparison:

```sh
python3 bench/run.py --work-dir "$bench_dir" --binary "$renderer" \
  --samples 3 --warmups 1 \
  --renderer mdviewer/terminal-tex \
  --renderer mdviewer/plain-pipe \
  --renderer mdviewer/terminal-math
```

Set `bench_dir` to the generated external corpus directory and `renderer` to the
release executable being measured. The generator, corpus hashes, initial raw
baseline, lockfile, and dependency set are unchanged by these optimizations.
Large inputs, raw local results, profiling instrumentation, and generated output
remain outside the repository.
