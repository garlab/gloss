# Text wrapping and benchmark timing

ASCII wrapping now avoids per-grapheme iteration when byte boundaries and cell
boundaries coincide. In an interleaved comparison, long-line rendering takes
about half as long, prose improves by 19–22%, and the 10 MiB mixed document
improves by 14%. Output, memory use, and executable size remain effectively
unchanged. These measurements supplement the fixed
[initial baseline](performance-baseline.md); its files and samples are unchanged.

## Changes

- Scan ASCII runs for word separators without decoding or measuring every
  grapheme. Preserve CRLF, tab stops, other whitespace, styles, and link targets.
- Split oversized ASCII words by the available cell count. Non-ASCII text keeps
  the existing Unicode grapheme and width handling, including accents and emoji.
- Continue borrowing word fragments and allocating only the physical output
  lines. No dependency, cache, resource budget, or output format changed.
- Fix `bench/run.py` waiting in `select(0.01)` when both output streams have
  already closed but the process has not yet exited. It now attempts a
  nonblocking reap and sleeps for 0.2 ms only while the process is still alive,
  retaining timeout checks and `wait4` CPU/RSS accounting.

The runner records **harness version 2**. Older short-case wall times can include
up to 10 ms of empty-selector delay; CPU and RSS do not depend on that fix. Both
binaries below were measured with version 2, so removing the delay is not counted
as a renderer speedup. Keep old results as historical measurements rather than
rewriting them or treating a harness change as a binary improvement.

## Measurements

Measured on Apple M2 Pro / 16 GiB, macOS 26.4.1, Rust 1.99.0. Both release builds
use thin LTO, one codegen unit and stripped symbols, with the same locked registry
dependencies (`document-graphics` 0.1.1 and `katex-native` 0.1.0). The before build
is commit `e8f927b`; the after build adds the wrapping changes described here.

The terminal is a drained 100×40 PTY with 10×20-pixel cells, 80 content columns
and two-cell margins. Inputs come from `bench/generate.py`, with warm filesystem
caches. Each table entry is the median of seven fresh processes per binary after
one warm-up. Before/after order alternated on successive samples to reduce drift.
Raw samples and generated inputs remain outside the repository.

| Input | Before wall | After wall | Before CPU | After CPU | Before RSS | After RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Long lines, 1 MiB | 47.1 ms | 24.1 ms | 40.0 ms | 16.4 ms | 8.97 MiB | 8.91 MiB |
| Prose, 1 MiB | 66.1 ms | 53.9 ms | 56.1 ms | 43.4 ms | 9.56 MiB | 9.50 MiB |
| Prose, 10 MiB | 611.8 ms | 476.2 ms | 521.2 ms | 391.6 ms | 50.53 MiB | 50.48 MiB |
| Mixed, 10 MiB | 1,129.3 ms | 975.8 ms | 985.7 ms | 848.2 ms | 58.27 MiB | 58.38 MiB |

Long-line first output improves from 17.5 to 9.7 ms. First output for the prose
and mixed cases stays within 0.3 ms of the control. Both binaries are
**16,978,560 bytes**. SHA-256:

- Before: `490c2912bd6de51a6b82a822db3d0ebabe69949c7d968d11461aeeb873f92d90`
- After: `13cdc68769bf238597e24f832f9072e18d703ef5e4128e14882f7e459fb10a94`

A separate 12-case pass used one warm-up and five samples for CommonMark, mixed
documents, long lines, prose, code, tables, math, and plain pipes. All cases
completed successfully with the expected image counts and no diagnostics.
Unchanged paths stayed below the
[regression review triggers](feature-design.md#resource-and-performance-requirements):
plain-pipe wall medians changed by −1.0% to +2.5%, and the 1,000-equation control
by +5.9%. Small differences remain sensitive to scheduling and PTY draining;
they are not portable guarantees. No terminal GPU, image-decoding or scrollback
cost is measured by this harness.

## Validation

All 60 Rust tests and 53 Unix PTY tests pass, as do formatting and Clippy. The
wrapping regression checks ASCII scanning against the existing Unicode path,
including whitespace, narrow widths, styles and links. Existing tests cover
oversized words, mixed image/text lines, accents and emoji.

An external comparison found byte-identical before/after output in 20
terminal/pipe cases, including light-theme and narrow-width fixtures. PTY output
translation was disabled for this comparison to avoid kernel-inserted carriage
returns. Exact source exports also match their input bytes. No visual layout or
terminal image-placement change is intended.

The harness fix was checked with a child that closes stdout/stderr before
exiting. An assertion against selecting an empty descriptor set fails with the
previous runner and passes with version 2. A second child that closes its streams
and hangs still times out, is killed, and is reaped with resource usage recorded.

## Reproduction

Generate the external corpora using the instructions in the
[baseline](performance-baseline.md), then run each saved release binary with the
same corrected runner. Save `results.json` before the next invocation overwrites
it. This command measures one binary; alternate binary order across runs when
checking small differences.

```sh
python3 bench/run.py --work-dir "$bench_dir" --binary "$renderer" \
  --samples 7 --warmups 1 --renderer gloss/terminal-tex \
  --corpus long-lines-1m.md --corpus prose-1m.md \
  --corpus prose-10m.md --corpus mixed-10m.md
```

## Remaining graphics work

- **Dense chained Mermaid flowcharts and tall sizing:** subsequent work adds
  contiguous image bands and optimizes routing comparisons without changing the
  search tiers or generated SVG. See [Mermaid support](mermaid.md) for current
  behavior, measurements, and the published backend versions.
- **Math padding:** images are rasterized before transparent cell padding is
  added. Omitting that padding could reduce allocation/copying, compression and
  transport work; it does not remove the original equation-layout/rasterization
  cost. Pixel offsets need direct-output and pager placement/cropping tests in
  Ghostty and Kitty. A percentage of transparent pixels is not an equivalent
  percentage of total rendering time saved.

These are separate changes with graphics-specific validation. This optimization
does not change diagram sizing, routing, equation dimensions or graphics protocol.
