# Syntax highlighting

Fenced code now retains its language and source span as a separate layout block.
Colored terminal output highlights one logical code block before wrapping it.
This preserves multiline syntax while avoiding a whole-document token model.
The [README](../README.md#terminal-rendering) lists supported languages and aliases.

The dependency-cost and performance-comparison sections below describe the
initial highlighting implementation.
Subsequent [rendering optimizations](performance-optimizations.md) reduce its
time, memory, output volume, and binary size while retaining the same languages
and highlighting limits.

## Implementation and limits

The terminal-independent Tree-sitter adapter maps query captures to semantic
token kinds. The renderer maps those kinds to dark/light colors. Overlapping
captures are resolved by a boundary sweep: inner nodes override outer nodes,
earlier query patterns win identical ranges, and plain captures can clear a
surrounding color. Scratch memory scales with captures, not source bytes.

Only used languages compile their bundled queries. A rendering invocation keeps
those queries and parser scratch space; each block's tree, captures, and styled
runs are released after use. There is no persistent syntax cache, helper process,
runtime grammar download, editor dependency, or additional checkout.

| Limit | Behavior |
| --- | --- |
| 128 KiB decoded code per block | Larger blocks retain the ordinary code panel. |
| 16,384 query captures per block | Capture-heavy blocks fall back to uncolored code. |
| 128 embedded regions per Dockerfile block | More regions retain the complete uncolored block. |
| 1,024 in-progress query matches | Exceeding the query cursor limit causes fallback. |
| 25 ms parse/query/span work per block | Cooperative callbacks and elapsed-time checks abandon expensive blocks. |
| 8 MiB attempted code per invocation | Further blocks remain uncolored. |
| 1 s accumulated highlighter time per invocation | Further blocks remain uncolored; this excludes ordinary layout and terminal I/O. |

Time limits are cooperative, not operating-system deadlines. Compiling a bundled
query is not interruptible; its time counts toward the invocation budget. The
fixed language/query set, input-size cap, capture cap, and query match limit
bound the work presented to the parser and span resolver. These are not a claim
of a hard process-wide memory ceiling.

Unknown languages, unlabeled/indented blocks, and exhausted limits keep all code
content. Incomplete snippets use grammar error recovery where possible.
Embedded-language highlighting is limited to the Dockerfile support below.
A lexical fallback is not implemented.

`--no-highlight` bypasses the highlighter without disabling the rest of the
theme. `NO_COLOR`, plain pipes, `--output text`, and `--output source` also
bypass it. Plain export uses the flat layout path so it does not pay for
independently owned fence metadata and per-block vectors. Exact source export
still bypasses parsing entirely.

## Dockerfile and Containerfile

The `dockerfile`, `containerfile`, and `docker` fence/language aliases share
[`tree-sitter-containerfile` 0.9.2](https://crates.io/crates/tree-sitter-containerfile),
including its published highlight and injection queries. A small leading query
colors complete instruction flags because the bundled query expects a named
child absent from those nodes. Grammar and query data remain in the dependency;
file detection, semantic span merging, terminal colors, and budgets remain here.

Instruction keywords, comments, variables, flags, strings, and ports
use the parent grammar. Shell commands and `RUN` heredocs use Bash. `COPY` heredocs
with JSON, YAML, or TOML destination extensions use the corresponding bundled
grammar. Every embedded region is parsed independently, so an unfinished shell
string cannot affect the next instruction. Unsupported embedded languages keep
the parent's colors.

This adds one dependency and reuses the existing Bash/JSON/YAML/TOML grammars.
Queries still compile lazily. There is one nonrecursive injection layer; sorted,
disjoint UTF-8 ranges prevent repeated parsing of overlapping content. The
16,384-capture and 25 ms work limits apply to the whole block, including discovery,
embedded parsing, queries, and span merging. Query compilation counts toward the
invocation's time budget. Trees are released between regions, and span overlays
use a linear merge instead of storage proportional to every input byte.

### Incremental cost

A release comparison against `0b22557` used the repository's PTY benchmark routine:
100×40 cells, 10×20 pixels per cell, one warm-up and five interleaved measured
processes per case. Results are medians; emulator painting is excluded. Generated
inputs and raw measurements remain outside the repository.

| Input | Before | With Dockerfile support | Before RSS | After RSS |
| --- | ---: | ---: | ---: | ---: |
| Ordinary Markdown, 1 MiB | 92.4 ms | 92.1 ms | 16.72 MiB | 16.78 MiB |
| Standalone YAML, 16 KiB | 13.9 ms | 13.6 ms | 6.77 MiB | 6.88 MiB |
| Dockerfile fixture, shell and JSON/YAML/TOML heredocs | 4.3 ms | 10.3 ms | 3.06 MiB | 11.17 MiB |
| Dockerfile, 64 shell commands | 3.5 ms | 7.5 ms | 3.11 MiB | 10.45 MiB |
| Dockerfile, 129 commands (region-limit fallback) | 3.5 ms | 4.6 ms | 3.08 MiB | 5.11 MiB |
| Dockerfile, 1 MiB (size-limit fallback) | 157.4 ms | 158.1 ms | 5.28 MiB | 5.34 MiB |

The release executable grew from 17,112,336 to 17,245,152 bytes, an increase of
129.7 KiB (0.8%). The embedded-language fixture initializes four existing grammars
in addition to the new one. `--no-highlight` renders it in 3.5 ms at 3.14 MiB peak
RSS. Markdown, YAML, disabled highlighting, and exhausted-limit cases retained
their previous output sizes; terminal tests also verify complete source and
matching plain output with and without colors.

## Dependency cost

The initial set covers Bash, Go, JavaScript/JSX, JSON, Python, Rust, TOML,
TypeScript/TSX, and YAML. The lockfile adds 23 packages, including the parser,
grammar bindings, query predicate support, and build dependencies.

Tree-sitter 0.27.1 and tree-sitter-language 0.1.9 require Rust 1.90, so the declared
minimum was raised from 1.88. Validation used Rust 1.99.0; a dedicated minimum
toolchain CI job remains part of the distribution work.

The initial release executable grew from 5,167,008 bytes
(4.93 MiB) to 13,455,248 bytes
(12.83 MiB). The measured implementation binary has
SHA-256 `aaaa46048f10087872c6e5291f77e9bf246baa6ce5513490d1dbb321c3d9a28a`.

The native adapter was selected because semantic capture handling was reusable
and its parse/query callbacks support bounded work. No direct Syntect comparison
was performed. Reconsider the grammar set or engine if additional language
coverage imposes excessive startup, memory, or binary costs.

## Performance comparison

These are the initial same-session release comparisons against the `0d4c347`
binary, using the [baseline method](performance-baseline.md): Apple M2 Pro,
16 GiB RAM, macOS 26.4.1, Rust 1.99.0, thin LTO, warm filesystem caches, one warm-up
and three fresh measured processes per case. Results are medians. Terminal runs
use TeX math unless explicitly labeled as graphics.

Each of the before, disabled, and enabled configurations completed 20 cases,
including 80 processes with warm-ups. Every run exited successfully, produced
no diagnostics, and emitted the expected number of math images. This compares
the syntax implementation with the old binary; the earlier Glow comparison is
preserved separately. Actual emulator painting is excluded.

| Input | Before | Disabled | Enabled | Before RSS | Enabled RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| CommonMark, 200 KiB | 31.49 ms | 31.55 ms | 30.97 ms | 9.62 MiB | 7.86 MiB |
| Mixed, 1 MiB | 122.52 ms | 122.89 ms | 179.17 ms | 36.88 MiB | 32.97 MiB |
| Mixed, 10 MiB | 1,163.09 ms | 1,150.63 ms | 1,622.29 ms | 332.12 MiB | 253.41 MiB |
| Prose only, 1 MiB | 107.32 ms | 109.12 ms | 108.12 ms | 31.62 MiB | 31.83 MiB |
| Prose only, 10 MiB | 1,067.14 ms | 1,042.42 ms | 1,032.90 ms | 325.66 MiB | 325.89 MiB |
| Language sample | 18.12 ms | 18.33 ms | 72.99 ms | 5.88 MiB | 20.80 MiB |
| Multiline comment | 27.71 ms | 27.83 ms | 37.80 ms | 5.20 MiB | 9.67 MiB |
| Oversized fence | 68.98 ms | 67.21 ms | 67.50 ms | 7.97 MiB | 8.17 MiB |
| Capture-heavy JSON | 6.56 ms | 6.60 ms | 24.82 ms | 7.20 MiB | 13.42 MiB |
| Plain pipe, mixed 10 MiB | 148.10 ms | 142.13 ms | 138.82 ms | 230.77 MiB | 230.88 MiB |
| Math graphics, 1,000 equations | 679.72 ms | 761.68 ms | 674.32 ms | 9.86 MiB | 10.00 MiB |

The prose inputs contain no fenced code. The language sample repeats
`tests/fixtures/code.md` 100 times. Additional cases exercise long multiline
comments, an over-limit fence, and a capture-heavy JSON block. The original ten
corpus hashes and the initial baseline artifact are unchanged.

Separating code blocks lets the renderer release temporary line structures between
blocks. On the mixed 10 MiB input, terminal peak RSS drops from
332.12 to 253.41 MiB with highlighting. The highlighter adds
2.64 MiB over the new disabled path for this input. Highlighting raises
wall time from 1.16 to 1.62 seconds, and terminal output from
24.09 to 32.66 MiB. These measurements separate added highlighting work from
changes to the existing layout path.

Prose-only memory stays effectively unchanged. Plain export retains its earlier
allocation behavior and does not initialize syntax resources. The language sample
uses 20.80 MiB with all of its languages active versus
5.59 MiB disabled, showing why the used grammar set matters.

The initial three-sample disabled math run crossed the timing review trigger.
A seven-sample repeat measured the unchanged 1,000-equation graphics path at
695.56 ms before, 695.60 ms disabled, and 671.55 ms enabled;
the regression did not reproduce. The 100-equation control was also repeated.
Both controls keep syntax resources uninitialized. These repeats add 48
successful processes including warm-ups to the comparison.

The disabled/absent/export/math paths were checked against the feature design's
review triggers. Highlighting increases colored code output and work; measure
that separately from unchanged paths. The comparison is a small local sample,
and the baseline's PTY/emulator limitations still apply.

To reproduce, generate the external inputs as described in the baseline and run
`bench/run.py` once normally and once with `--no-highlight`, saving results
between runs. Select `--corpus 'prose-*'` or `--corpus 'code-*'` for focused
checks. Keep large inputs and raw local results outside the repository.

## Validation

Rust tests cover grammar queries, token precedence, multiline syntax, Unicode,
limits, source ranges, and unchanged exports. Pseudo-terminal tests cover
dark/light colors, disabled and unsupported paths, wide/narrow wrapping,
list/quote boundaries, control filtering, and existing math behavior. Formatting
and Clippy checks pass. Actual Ghostty/Kitty visual checks remain necessary for
font-dependent appearance; PTY tests do not establish visual compatibility.
