# Architecture

mdviewer separates the original document, layout, math rasterization, and terminal
output. The Rust library API is not yet stable; these boundaries guide further
implementation.

## Data flow

```text
UTF-8 Markdown
├── exact source output
└── Document: original source + parsed events + byte ranges
    └── Layout: styled text runs + display math blocks
        ├── plain text export with source TeX
        ├── styled terminal text with source TeX
        └── cell-aware lines + math rasters → Kitty graphics and native text
```

The CLI chooses an output mode before rendering. Non-terminal stdout always uses
text export unless exact source output was requested. Math is laid out only when
graphics are selected.

## Module boundaries

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Arguments, input, output policy, errors, and diagnostics. |
| `src/document.rs` | CommonMark events, original Markdown, and source ranges; normalize fenced math. |
| `src/layout.rs` | Convert events into styled text runs and display math blocks. |
| `src/layout/lines.rs` | Wrap graphemes by cell width and measure inline baselines, independently of escape sequences. |
| `src/math.rs` | Adapt `katex-native`, fit bounded rasters, combine equation tags, and track numbering. |
| `src/terminal.rs` | Observe stdout, environment hints, multiplexers, and cell geometry. |
| `src/export.rs` | Generate readable plain text or return the original source. |
| `src/render/mod.rs` | Coordinate rendering and per-formula fallbacks. |
| `src/render/text.rs` | Emit text styling and filter input control characters. |
| `src/render/inline.rs` | Arrange native text and inline images within reserved terminal rows. |
| `src/render/kitty.rs` | Pad images to cell boundaries and transmit chunked graphics commands. |

`katex-native` owns TeX parsing, equation layout, fonts, and rasterization.
Terminal-specific behavior belongs here. Additional graphics protocols should
reuse the document model and layout rather than introduce separate parsers.

## Contracts to preserve

- **Source remains authoritative.** Original UTF-8 bytes survive independently of
  pixels and styles. Runs retain source spans and original TeX. Exact source
  output preserves line endings and final-newline state.
- **Text output is independent of graphics.** Plain export contains readable text
  and TeX without terminal controls or viewport-width wrapping. It never reads
  back rendered output. Diagnostics go to stderr.
- **Rendered input is filtered.** User-provided text cannot emit terminal control
  sequences through styled or plain rendering. Exact source output deliberately
  bypasses filtering.
- **Failures stay local.** Invalid formulas retain their source while later text
  and equations continue rendering. Image dimensions and raster allocation are
  bounded. Inline rendering does not advance display equation numbering.
- **Layout respects terminal cells.** Wrapping preserves graphemes and keeps
  images whole. Inline layout reserves rows above and below the baseline. Images
  are padded to cell dimensions to avoid stretching, and space is reserved before
  drawing near the bottom of the terminal.
- **Terminal discovery does not consume document input.** Current discovery uses
  environment hints and geometry only. Any future query handling must preserve
  stdin and user input and have bounded timeouts.

Source ranges currently identify whole runs. Precise selection within decoded
entities needs a finer mapping. Font baselines and missing cell dimensions are
estimated, which requires visual validation across terminals and fonts.

## Validation

`tests/integration.rs` covers parsing, exact source and readable text exports,
control-character filtering, formula fallbacks, numbering, and graphics payloads.
Unit tests cover line measurements, wrapping, and terminal hints.
`tests/terminal_smoke.py` exercises the CLI through Unix pseudo-terminals and
checks image/text placement. Neither suite establishes visual compatibility with
a particular terminal version.

Use the fixtures for manual checks at normal and narrow widths, with tall
equations, light and dark backgrounds, and output near the bottom of the screen.
Check text selection and selectable TeX as well as appearance. Development
commands are in the [README](../README.md).
