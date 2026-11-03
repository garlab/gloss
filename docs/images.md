# Local images

Markdown PNG, JPEG, and SVG references can render through Kitty graphics. Math
and ordinary images share straight RGBA pixels, cell padding, row reservation,
and transport; equation baselines and TeX remain separate metadata. No change to
the math library is required.

`--images auto` uses the existing Ghostty/Kitty environment hints. `--images off`
skips image file access. `--images kitty` enables another compatible terminal,
subject to the existing pipe, multiplexer, dumb-terminal, and geometry rules.
These options are independent of `--math`. A nonempty `NO_COLOR` affects text
styling, not image pixels.

Paths resolve relative to the Markdown file, or the working directory for stdin.
`--base-dir` overrides that base. Percent-encoded UTF-8 paths are accepted.
Only regular local files with `.png`, `.jpg`, `.jpeg`, or `.svg` extensions are
opened. URLs, data URIs, query/fragment suffixes, and nested SVG resources are
unsupported; rendering never fetches a remote image.

Images shrink to fit the reading column and viewport while preserving aspect
ratio; they are never enlarged. JPEG orientation is applied before fitting.
Every image has a selectable `[image: alt] (target)` caption. Failure, unsupported
content, and budget exhaustion leave the caption intact and do not stop later
text. Captions retain the original reference rather than a resolved absolute
path. Inline image references become separate blocks. Table images remain text
references in this version.

Plain pipes, `--output text`, and exact source export preserve their previous
representations and bypass image blocks, file access, decoding, and caches.
Source export remains byte-for-byte Markdown. Selecting image pixels cannot
recover their contents; the caption and original source remain available for
copying.

## Decoding and limits

The loader is created only when an enabled image block is encountered. PNG/JPEG
decoding uses the optional `images` API in `document-graphics`, backed by
`image` with only those codecs enabled. SVG uses its strict document renderer
with bundled fonts. mdviewer does not enable native codecs or the broader image
worker. Image decoding adds no helper process or persistent cache. File-access
policy, invocation budgets, fitting bounds and cache eviction stay in mdviewer.

| Limit | Behavior |
| --- | --- |
| 8 MiB encoded PNG/JPEG; 256 KiB SVG | Reject larger files before reading their contents. |
| 8,192 pixels per source edge; 4,000,000 source pixels | Check bitmap dimensions before full decoding; bound intrinsic SVG dimensions. |
| 32 MiB bitmap decoder allocation budget | Combine codec limits with a decoded-byte preflight; this is not a process RSS ceiling. |
| 1,000,000 fitted pixels | Also fit the reading width and viewport minus two rows. |
| 4,096 file-open attempts; 64 MiB of file reads per invocation | Include repeated references; reject further work when exhausted. |
| 64 decode attempts; 2 seconds cumulative decode time | Failures count; cached pixels can still be used. Time is checked between decodes. |
| 8 MiB cache; at most 32 entries | Count encoded source and fitted RGBA bytes; evict least recently used entries. |
| 64 MiB total cell-padded RGBA output | Count every placement, including cache hits; later images retain captions. |

File reads use fixed-size buffers and reject growth beyond the observed size.
Unix opens are nonblocking and reject nonregular files. PNG/JPEG codecs and SVG
rasterization run in-process: the cumulative time budget cannot interrupt a
single decode and is not a hard deadline. Source input, layout, temporary decoder
storage, cached pixels, padding, and compression have separate allocations.
Base64 transport uses a fixed 4 KiB output buffer.
The limits bound added image work rather than total process memory or filesystem
latency.

Cache identity includes exact encoded content, format, and requested fitting
bounds. A content hash accelerates lookup, with byte equality checked before a
hit. Files are reread, so aliases can reuse pixels and changed files invalidate
them. Renderer code and fonts are fixed within the invocation; images keep their
original colors. Cache hits avoid decoding but still pad and transmit each
placement. Transport now compresses beneficial payloads before base64 encoding;
terminal-side image reuse remains follow-up work. See the
[streaming and graphics measurements](streaming-and-graphics.md).

The output budget is now shared by local Markdown images and
[native Mermaid diagrams](mermaid.md); existing math output has its own
fitting/row limits. Base64 expands the pixel payload by roughly one third,
and captions/protocol commands add bytes. The budget does not bound emulator
scrollback or GPU memory.

## SVG subset and fonts

Accepted content includes groups, paths, rectangles, circles, ellipses, lines,
polygons, linear/radial gradients, clip paths, text, and local styles. XML has a
4,096-node limit, nesting is bounded, path/point attributes total at most 64 KiB,
and text/style nodes total at most 32 KiB. The whole SVG remains limited to
256 KiB. DTDs are rejected.

Filters, masks, patterns, markers, `use`, nested images, foreign HTML, scripts,
and external style/font resources fall back to the image reference. Resource
URLs must be local fragment references; the SVG loader also disables image
resolvers. This is intentionally a constrained SVG implementation, not full
browser rendering.

SVG text lazily registers eight already bundled KaTeX font faces: main,
sans-serif, and monospace families with their available variants. Registration
borrows the static font bytes, corrects italic metadata, and does not scan system
fonts. Font substitution can change metrics; unavailable glyphs cause the image
to fall back instead of silently dropping a label. Broader font coverage is
future work. Shapes-only SVGs do not initialize a font database.

## Validation

Rust tests cover content cache reuse/invalidation, percent-encoded paths,
transparency, JPEG orientation, lazy fonts, missing glyphs, SVG restrictions,
allocation/work/output limits, retained image source ranges, and unchanged
exports. Pseudo-terminal tests cover file-relative and stdin bases, independent
math/image modes, unsupported terminals, fitting, captions, nested list/quote
context, missing files, and complete references after fallbacks.

The shared line painter also fixes table separator gaps beside tall equations:
separators and quote rails now span all reserved rows above and below the text
baseline. A screen-model regression test checks every border cell, including
with color disabled and a short viewport. These tests do not establish visual
compatibility with a real emulator or font. Use `tests/fixtures/images.md` and
`tests/fixtures/tables.md` for manual Ghostty/Kitty checks.

## Measurements

The control is `4d81700`, the committed table implementation. The initial
[performance baseline](performance-baseline.md) and raw artifact remain fixed.
Measurements use the same method: Apple M2 Pro, 16 GiB RAM, macOS 26.4.1,
Rust 1.99.0, release profile with thin LTO, one codegen unit, and stripped
symbols. Filesystem caches are warm, with one warm-up and three fresh measured
processes per case. Values are medians. A drained 100 by 40 PTY reports 10 by
20-pixel cells; the reader is 80 columns wide. No emulator paints the output.

All 47 cases per build completed without diagnostics and with expected image
counts: 376 processes including warm-ups. The original 21 corpus hashes remain
unchanged; nine image inputs add 18 terminal/pipe cases. Large inputs, generated
assets, and raw local results remain outside the repository.

| Unchanged path | Before time | Images build time | Before RSS | Images build RSS |
| --- | ---: | ---: | ---: | ---: |
| CommonMark, 200 KiB | 18.85 ms | 18.99 ms | 5.86 MiB | 6.14 MiB |
| Mixed, 1 MiB | 117.41 ms | 127.35 ms | 25.22 MiB | 25.42 MiB |
| Plain pipe, mixed 10 MiB | 89.30 ms | 87.73 ms | 138.61 MiB | 138.62 MiB |
| Single table, 10 MiB | 701.65 ms | 698.99 ms | 237.02 MiB | 237.22 MiB |
| Graphics, 5,000 equations | 3,612.43 ms | 3,614.48 ms | 15.77 MiB | 15.92 MiB |
| Table graphics, 100 equations | 19.83 ms | 20.64 ms | 6.81 MiB | 7.09 MiB |

| Image input | Before, references only | Images enabled | Enabled RSS | First byte | Output |
| --- | ---: | ---: | ---: | ---: | ---: |
| PNG, 120 by 60 | 2.88 ms | 3.86 ms | 3.28 MiB | 3.14 ms | 0.04 MiB |
| JPEG, 120 by 60 | 3.24 ms | 3.43 ms | 3.39 MiB | 2.97 ms | 0.04 MiB |
| SVG shapes, 120 by 60 | 3.02 ms | 3.57 ms | 3.75 MiB | 2.89 ms | 0.04 MiB |
| SVG text, 360 by 120 | 2.99 ms | 6.14 ms | 5.23 MiB | 3.62 ms | 0.22 MiB |
| 100 identical PNG references | 3.42 ms | 43.25 ms | 3.66 MiB | 3.32 ms | 3.69 MiB |
| 32 unique SVGs | 3.21 ms | 17.14 ms | 5.19 MiB | 3.06 ms | 1.18 MiB |
| PNG, 2,000 by 2,000, fitted down | 2.99 ms | 50.79 ms | 26.08 MiB | 20.18 ms | 2.95 MiB |
| Invalid/unsupported inputs, then valid PNG | 3.23 ms | 4.12 ms | 3.69 MiB | 3.47 ms | 0.04 MiB |
| 1,000 references, 400 by 200 PNG | 6.10 ms | 929.72 ms | 6.42 MiB | 4.76 ms | 85.40 MiB |

The last input renders 209 images, then keeps all remaining captions once the
64 MiB padded-pixel budget is exhausted. It demonstrates bounded output but also
the transport cost of repeated pictures. Caching does not remove that cost. The
large-source PNG measures decoding memory separately from small retained pixels.
Each SVG text process starts with an empty font database; repeated references
exercise an invocation-local cache, not a warm persistent cache. First observed
output can precede useful content and does not measure completed screen paint.

Seven-sample repeats of the two largest prose/mixed controls account for 32
additional processes and reduce the effect of run-to-run variation:

| Terminal input | Before time | Images build time | Before RSS | Images build RSS |
| --- | ---: | ---: | ---: | ---: |
| Mixed, 10 MiB | 1,115.92 ms | 1,115.30 ms | 177.25 MiB | 177.58 MiB |
| Prose only, 10 MiB | 608.28 ms | 588.46 ms | 167.20 MiB | 167.41 MiB |

First output for mixed 10 MiB is 88.60 versus 90.95 ms. The fixed initial
baseline was about 1.16 seconds and 332 MiB on this input, so the earlier memory
improvement is retained. Ordinary-document costs remain dominated by parsing,
layout, and text output.

Eleven disabled-image cases add 44 successful processes: the nine image inputs
and both large ordinary-document controls. Small image references take about
2.94–3.68 ms with 2.86–3.20 MiB RSS; the 1,000-reference input takes 5.75 ms and
4.22 MiB. No image payloads are emitted. All nine image pipe cases similarly
retain text-only costs (about 3.0–3.7 ms), with no image decoding.

No unchanged path in the comparisons crossed the feature design's
timing, first-output, or RSS review triggers. Renderer RSS excludes emulator
memory; PTY timing excludes emulator decoding, GPU work, and scrollback. These
small local samples are reference measurements, not portable guarantees.

The release executable grows by 469,968 bytes, from 12,670,416 to 13,140,384 bytes
(12.08 to 12.53 MiB, about 3.7%). Its SHA-256 is
`ce0121e3b7e9c5c30271ec26b33526f4a122b737d3bae91dc368e2a742edfef2`.
Seven packages are added to the lockfile: `image`, `png` 0.18, `zune-jpeg`,
`zune-core`, `byteorder-lite`, `moxcms`, and `pxfm`. `resvg` and `roxmltree` become
direct dependencies but were already in the math dependency tree. No fonts,
browser runtime, or additional private dependency are added.

All 41 Rust tests, 33 pseudo-terminal tests, formatting, and Clippy pass.
External output comparisons check unchanged text/styles/graphics and all 30
plain and exact-source exports.

Generate inputs using the existing [benchmark instructions](performance-baseline.md).
The generator now adds `images-*` documents and deterministic external assets.
Select them with `--corpus 'images-*' --renderer mdviewer/terminal-images`.
Compare `--image-mode auto` with `--image-mode off`; use `--image-mode legacy`
for older binaries that predate the `--images` flag. The runner checks asset and
document hashes as well as expected image counts. Include `mdviewer/plain-pipe`
to check export costs.

## Shared decoder extraction

The 0.1.5 integration moves PNG/JPEG decoding into the optional `images` API in
`document-graphics`. Existing image bounds and strict SVG policy remain intact.
Before/after release binaries produced identical output for image, inline-math
and Mermaid fixtures at 24 and 100 columns. Alternating measurements showed no
meaningful timing change; the binary grew by 48 bytes. See the
[shared extraction report](https://github.com/garlab/document-graphics/blob/main/docs/image-extraction.md)
for methodology, platform limits and release validation.
