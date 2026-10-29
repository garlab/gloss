# Initial performance baseline

These measurements establish a reference before adding syntax highlighting,
aligned tables, general images, and Mermaid. They describe the current feature
set, not a feature-equivalent contest with Glow. Glow already does syntax
highlighting and table layout that mdviewer did not perform in these runs.

The full samples, CPU time, output sizes, validity checks, and input hashes are in
[baseline-0d4c347.json](../bench/baseline-0d4c347.json). Large inputs and generated
outputs are deliberately not stored in this repository.

## Versions and method

| Item | Baseline |
| --- | --- |
| mdviewer | `0d4c347`, clean source tree |
| katex-native | `982f130`, clean source tree |
| Glow | 3.0.0 |
| Host | Apple M2 Pro, 16 GiB RAM, macOS 26.4.1 |
| Rust | rustc 1.99.0 (b940084d7 2026-09-28) |
| Build | `cargo build --release --locked --offline`; thin LTO, one codegen unit |
| mdviewer binary | 5,167,008 bytes (4.93 MiB) |
| Binary SHA-256 | `6a43159cff4106ab621d022699cfe34694b64e7cd2e163faa1c79604dbc77a35` |
| Sampling | One warm-up, then three fresh measured processes per case; warm filesystem caches |
| Terminal | Drained Unix PTY, 100 columns × 40 rows, 10 × 20 pixel cells |
| Layout | mdviewer: 80 content cells and 2-column margins; Glow: width 84, dark style, empty config |
| Timeout | 45 seconds per process |

All 20 cases completed: 80 processes including warm-ups, 60 measured samples.
Every run exited successfully, emitted no stderr, and produced the expected
number of graphics transmissions. Tables below show the median of the three
measured samples; MiB means 1,048,576 bytes.

Wall time includes process startup, parsing, layout, rasterization where enabled,
encoding, and draining output. First-output time is the first byte observed by
the harness, which can precede useful content. CPU and peak RSS come from
`wait4` for the renderer process. Filesystem caches were not flushed.

No real terminal painted the output. Terminal-side image decoding, GPU work,
scrollback storage, and perceived responsiveness are excluded. RSS does not
include an emulator or independently measured helper process tree.

## Text in a terminal

mdviewer used `--math tex`. Glow used `--config <empty-config> -s dark -w 84`.

| Input | mdviewer time | Glow time | mdviewer peak RSS | Glow peak RSS |
| --- | ---: | ---: | ---: | ---: |
| CommonMark spec, 200 KiB | 31.65 ms | 3,085.14 ms | 9.61 MiB | 66.52 MiB |
| Mixed, 257 KiB | 33.62 ms | 512.92 ms | 11.95 MiB | 74.52 MiB |
| Mixed, 1.00 MiB | 122.26 ms | 2,000.06 ms | 36.78 MiB | 224.22 MiB |
| Mixed, 5.00 MiB | 587.84 ms | 10,625.94 ms | 168.55 MiB | 820.86 MiB |
| Mixed, 10.00 MiB | 1,160.49 ms | 23,369.87 ms | 331.88 MiB | 1,555.84 MiB |
| Long lines, 0.98 MiB | 76.94 ms | 221.25 ms | 42.95 MiB | 40.73 MiB |

| Input | mdviewer first output | Glow first output | mdviewer output | Glow output |
| --- | ---: | ---: | ---: | ---: |
| Mixed, 1.00 MiB | 14.91 ms | 1,769.93 ms | 2.42 MiB | 25.26 MiB |
| Mixed, 10.00 MiB | 111.20 ms | 21,053.46 ms | 24.09 MiB | 250.55 MiB |

The mixed inputs combine headings, nested lists, quotes, tasks, tables, Rust/JSON
fences, links, footnotes, Unicode, and long paragraphs. The CommonMark file is the
official 0.31.2 specification rendered as one document; this is not a CommonMark
conformance test. The long-line input combines a very long paragraph with an
unbroken 512 KiB word.

Both time and retained memory increase roughly with mixed-document size. The
10 MiB memory result is substantial despite the relative speed advantage.
The long-line case also shows that lower memory use than Glow is not universal.
Differences in output volume and implemented features contribute to the timing
gap; these measurements do not isolate parser performance.

## Native math graphics

mdviewer used `--math kitty`. Each input contains 75% inline and 25% display
equations, with a mix of fractions, matrices, sums, integrals, and tagged math.

| Equations | Time | First output | Peak RSS | Terminal output |
| --- | ---: | ---: | ---: | ---: |
| 100 | 65.22 ms | 3.68 ms | 7.80 MiB | 3.23 MiB |
| 1,000 | 647.44 ms | 4.65 ms | 9.75 MiB | 34.29 MiB |
| 5,000 | 3,405.69 ms | 7.25 ms | 18.98 MiB | 183.62 MiB |
| 1,000, repeated values | 552.52 ms | 4.64 ms | 9.41 MiB | 30.42 MiB |

There is no math raster cache in this revision. The repeated-value input renders
smaller formulas and emits fewer bytes; its faster result is not evidence of
cache reuse. Graphics output includes raw RGBA data encoded in Kitty commands.
Output bandwidth and terminal-side storage need attention alongside CPU time.

## Plain output and TeX fallback

| Mode and input | Time | Peak RSS | Output |
| --- | ---: | ---: | ---: |
| Plain pipe, mixed 1 MiB | 17.58 ms | 26.59 MiB | 0.95 MiB |
| Plain pipe, mixed 10 MiB | 142.18 ms | 230.80 MiB | 9.47 MiB |
| Plain pipe, 5,000 equations | 8.96 ms | 13.53 MiB | 0.47 MiB |
| Terminal TeX, 5,000 equations | 55.66 ms | 13.81 MiB | 0.97 MiB |

Plain pipes used `--output text` and produced no terminal images. Optional
rendering features must preserve this independent export path.

## Reproducing and extending

Use Unix and Python 3, a release build, Glow, and the sibling math dependency.
The small [generator](../bench/generate.py) and [runner](../bench/run.py) keep
corpora and new results in an external scratch directory:

```sh
bench_dir="$(mktemp -d)"
mkdir -p "$bench_dir/corpora"
curl -L --fail \
  https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt \
  -o "$bench_dir/corpora/commonmark.md"
python3 bench/generate.py --work-dir "$bench_dir"
cargo build --release --locked --target-dir "$bench_dir/build"
python3 bench/run.py \
  --work-dir "$bench_dir" \
  --binary "$bench_dir/build/release/mdviewer" \
  --binary-revision "$(git rev-parse HEAD)" \
  --profile "cargo build --release --locked" \
  --hardware "CPU model and RAM capacity"
```

The generator verifies the pinned CommonMark checksum and records all corpus
sizes and hashes. It does not download anything itself. The runner verifies
inputs, saves individual samples and medians to `results.json`, records the binary
hash and local source revisions/dirty flags, and fails on invalid runs. It bounds
diagnostics and kills the process group on timeout or interruption. Review saved
diagnostics and manually supplied metadata before sharing results.

For a short check, add `--samples 1 --warmups 0 --corpus mixed-1m.md`.
Repeat `--corpus` or `--renderer` to select cases; see `--help`. A new run
overwrites the scratch directory's results, so use separate directories or retain
the JSON when comparing versions.

The generator also provides prose-only controls and syntax-heavy, multiline,
oversized, and capture-heavy code inputs. Use `--no-highlight` with a current
binary to compare enabled and disabled syntax rendering. The original ten inputs
and saved baseline remain unchanged. See the
[syntax highlighting measurements](syntax-highlighting.md) for the first feature
comparison and its added cases.

The saved baseline is the original measurement artifact. The retained runner adds
portable arguments, metadata, input checks, cleanup, and validation; it is not an
exact copy of the original script. Validate harness overhead on an unchanged
binary before interpreting small differences. A clean checkout at the recorded
revisions and the recorded toolchain/profile is needed to reproduce the original
build; current-checkout runs measure the current code.

Follow the [feature design](feature-design.md#resource-and-performance-requirements)
when adding cases. Include absent/disabled/enabled paths and pathological inputs,
plus cold/cached runs and full process-tree measurements for external renderers.
Investigate allocation retention and graphics payload volume; the current
baseline is a reference to improve, not a resource budget to consume.
