# Native LaTeX preview

Status: in progress. The shared crate, `latex-preview`, uses a hand-written
scanner, chosen from the measurements under [Spike outcome](#spike-outcome).
gloss reads `.tex` files through it; the [README](../README.md#latex-documents)
describes current behavior. Remaining work is listed under
[Delivery](#delivery).

The intended feature is a fast, approximate preview of LaTeX documents: headings,
paragraphs, lists, emphasis, code, tables, theorems, references, and natively
rendered math, without a TeX installation or PDF generation. Two applications
would use it: gloss in the terminal, and the
[Plastext](https://github.com/garlab/plastext) desktop viewer, which currently
previews TeX by compiling it to PDF.

## Goals

- **Fast and close enough.** Render in proportion to the input with small
  constants, including very large documents, rather than reproducing TeX's
  output exactly.
- **Consistent with Markdown in gloss.** Equivalent constructs look the same:
  `\section{Introduction}` renders like `# Introduction`, `itemize` like a
  Markdown list, and `$...$` like Markdown math.
- **Closer to the PDF in a GUI.** A graphical consumer can approximate LaTeX
  typography (numbered sections, title block, theorem styles, captions) from the
  same parsed document.
- **One parser, two presentations.** LaTeX parsing, numbering, references, and
  math preparation are shared. Each application owns layout and styling.
- **Large documents and edits stay responsive.** Any part of a very large
  document can be parsed from a compact checkpoint, work is cancellable, and only
  results for the current source revision are published.
- **Nothing disappears silently.** Unsupported input stays visible as source and
  is reported, rather than being dropped or flattened.
- **Gloss's contracts hold.** Source stays authoritative, plain export remains
  readable, failures stay local, costs are bounded, and the Markdown path is
  unaffected. See [Contracts to preserve](architecture.md#contracts-to-preserve).

## Non-goals

- Executing TeX, loading packages or class files, or matching TeX's line and page
  breaking.
- An intermediate markup format. LaTeX is never converted to Markdown or Typst
  text that is then parsed again.
- PDF/EPS figures, TikZ, and BibTeX in the first increments. Figures in formats
  the image loader supports (PNG, JPEG, SVG) are in scope; others keep a
  reference.
- Replacing PDF compilation where exact output matters. A GUI can keep it as an
  explicit "exact" mode.

## Behavior before this design

`.tex` had no input format of its own, so it fell through to Markdown. On a short
article this produced:

- The preamble (`\documentclass`, `\usepackage`, `\newcommand`) printed as text.
- `\section{...}`, `\emph{...}`, and `\begin{itemize}` left literal, and `%`
  comments shown.
- CommonMark treating `\\` as an escaped backslash, which merges `align` rows and
  table rows into one line.
- Formulas that use preamble macros, such as `\R`, falling back to source because
  KaTeX does not know the macro.

Everything after parsing consumes one stream of events with source ranges:
layout, tables, images, math rasters, TeX fallbacks, plain export, and the pager.
`Document` already synthesizes this stream for standalone code and images (see
[Architecture](architecture.md#data-flow)). Reusing that pipeline is a goal, not a
constraint: formula context and richer table semantics need small interface
changes, described under [Gloss integration](#gloss-integration).

## Research

### Formula support

KaTeX, through `katex-native`, already renders gloss's math. The probe in
[`bench/latex`](#reproducing-the-measurements) checks the display constructs that
papers use most against `katex-native` 0.1.1 (katex-rs 0.3.0):

| Construct | Result |
| --- | --- |
| `equation`, `align` with `\nonumber` or `\tag`, `gather*`, `split` inside `equation`, `cases` | Renders. KaTeX's automatic numbering matches LaTeX for these cases. |
| `\operatorname`, `\boldsymbol`, `\bm`, `\text` | Renders. |
| `\label{...}` | Fails: undefined control sequence. Almost every numbered equation in a real paper has one. |
| `\eqref{...}` | Fails: undefined control sequence. |
| `\DeclareMathOperator` | Fails: undefined control sequence. |
| `multline`, `eqnarray`, `flalign` | Fail: no such environment. |
| `\mathbbm` | Fails: undefined control sequence. |
| `equation` with `\notag`; `align*` and `gather*` with `\tag{n}` on some rows; `\tag*{A}`; `gathered` inside `equation*` with one `\tag` | Renders, with no automatic numbers. These are the rewrite targets under [Equation numbering](#equation-numbering-and-rewrites). Row placement of the tags has not been checked visually. |

These failures are gaps in the katex-rs parser, not in `katex-native`'s layout.
The LaTeX front end prepares formulas before calling KaTeX, as described below.

Macros need no `katex-native` change. `layout_numbered` passes katex-rs's
`Settings` through, including `Settings::macros`:

- Inserting `\R` → `\mathbb{R}`, `\tr` → `\operatorname{tr}`, and
  `\norm` → `\left\lVert #1 \right\rVert` into the macro map rendered `x \in \R^n`,
  `\tr A`, and `\norm{x}`.
- Parsing `\newcommand{\abs}[1]{...}` and `\def\eps{...}` once with
  `global_group(true)` kept both definitions for later formulas.

KaTeX documents its scoping in its [options](https://katex.org/docs/options.html):
the `macros` object is modified by `\gdef` and `\global\let`, and also by `\def`,
`\newcommand`, and `\let` when `globalGroup` is enabled. With the default
`globalGroup: false`, definitions other than `\gdef` stay local to the formula,
as in LaTeX. Replaying `\newcommand` or `\def` through KaTeX with `globalGroup`
off therefore discards those definitions when the call returns.

### Parsers and converters

The survey covered Rust crates and repositories found on crates.io and GitHub.
Maturity was not a selection criterion, and forking was acceptable. The
requirements were exact source ranges, correct verbatim handling, local error
recovery, speed on large inputs, resumable parsing, and a license compatible
with gloss (MIT) and Plastext (MIT OR Apache-2.0).

| Candidate | What it is | License | Assessment |
| --- | --- | --- | --- |
| [tree-sitter-latex](https://github.com/latex-lsp/tree-sitter-latex) | Error-tolerant grammar used by editors | MIT | **Candidate.** Recognizes sectioning (as a nested tree), labels, references, citations, `\newcommand`, `\newtheorem`, counters, captions, graphics, includes, and verbatim/listing/minted environments. Exact byte ranges, local recovery, incremental reparsing, and highlight queries. No `\verb` and no macro expansion. Slower and heavier than MiTeX (see [Measurements](#measurements)). The generated parser is not committed upstream; crates.io has a third-party build, [`codebook-tree-sitter-latex`](https://crates.io/crates/codebook-tree-sitter-latex). |
| [MiTeX](https://github.com/mitex-rs/mitex) parser | Fast lexer and parser driven by a table of command argument shapes, producing a rowan tree | Apache-2.0 | **Candidate as a fork.** Fastest and small (about 4,000 lines for lexer, parser, and command table). Designed for math snippets: verbatim content is tokenized, `\verb` is not handled, unbalanced braces cascade silently, and macro expansion breaks source offsets. Last crates.io release 2024-06; the repository is active. |
| [tylax](https://github.com/scipenai/tylax) | Full-document LaTeX ↔ Typst converter (about 54,000 lines), built on MiTeX's parser | Apache-2.0 | **Reference and data only.** Broadest coverage (tables with `multicolumn`/`multirow`/`booktabs`, references, BibTeX, `siunitx`, macro engine), but its API is string to string with no source ranges. Using it means Typst text as an intermediate format. Its logic writes Typst strings directly, so a fork would rewrite most of it. |
| [rtex](https://crates.io/crates/rtex) | Recent LaTeX → PDF/HTML/DOCX converter | Apache-2.0 | Not a base. Its tree stores section titles and footnotes as plain strings and keeps no source positions. Its feature list is a useful checklist. |
| [unified-latex](https://github.com/siefkenj/unified-latex) | Well-designed LaTeX syntax tree with per-package argument tables | MIT (TypeScript) | Port argument tables only. The Rust wrapper [`unlatex`](https://crates.io/crates/unlatex) embeds a JavaScript engine. |
| texlab's parser, [RusTeX](https://github.com/FlexiFormal/RusTeX), [latex-to-html](https://crates.io/crates/latex-to-html), [latex_snippet](https://crates.io/crates/latex_snippet), [latex-ast](https://crates.io/crates/latex-ast) | Parsers and converters | GPL-3.0 or AGPL-3.0 | Excluded by license. RusTeX also reads a local TeX installation. |
| [pulldown-latex](https://crates.io/crates/pulldown-latex), latex2mathml, [tex2typst-rs](https://crates.io/crates/tex2typst-rs) | Math only | Various | Not needed: KaTeX covers math. |
| [Tectonic](https://github.com/tectonic-typesetting/tectonic), [TeXpresso](https://github.com/let-def/texpresso) | Real TeX engines | MIT | Exact output, but they need TeX resources and do not meet the speed and large-document goals. |

No permissively licensed Rust library provides the semantic layer with source
ranges: what each command means, numbering, cross-references, theorems, tables,
macro scoping, and math preparation. Converters that have this layer bake it
into their output format. The realistic choice is which syntax layer to start
from, plus which tables to borrow.

### Measurements

The corpus is the complete LaTeX sources of two public arXiv papers, pinned to
[1706.03762v7](https://arxiv.org/abs/1706.03762v7) and
[2005.14165v4](https://arxiv.org/abs/2005.14165v4): 374,049 bytes of `.tex`. The
files are joined in path order with newline separators, and
`\begin{document}`/`\end{document}` are removed so that one input covers every
file. A synthetic large input repeats that text 100 times (37,404,900 bytes).
Neither input is stored in the repository.

Each run is a fresh probe process that times one parse of an in-memory string,
excluding file reading and node counting. Peak resident memory is for the whole
process, measured with `/usr/bin/time -l`, and includes the input string. Values
are medians of seven runs on an Apple M2 Pro with Rust 1.99. The machine had a
load average of about 4 during the runs; repeated sessions differed by up to 4%.

| Parser | 374 KB | 37 MB | Throughput | Peak RSS, 374 KB | Peak RSS, 37 MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| tree-sitter-latex (`codebook-tree-sitter-latex` 0.6.1, tree-sitter 0.25.10) | 36.1 ms | 3.54 s | 10.6 MB/s | 17.1 MiB | 1,273 MiB |
| MiTeX (`mitex-parser` 0.2.4, default command table, macro expansion on) | 4.9 ms | 366 ms | 102 MB/s | 6.3 MiB | 247 MiB |

These numbers are for syntax trees only. Neither candidate is measured with
recovery, macro scoping, numbering, or checkpoint construction, which a real
first pass needs. Throughput claims for any proposed parser remain hypotheses
until the spike measures a complete first pass.

For scale, gloss's plain export of a 10 MiB Markdown document takes about 98 ms
(see [Markdown extensions](markdown-extensions.md#html-images)). At the measured
rate, tree-sitter-latex alone would need about 1 s for 10 MB of LaTeX.

On the real corpus both parsers agree on structure: 256 environments and 467
formulas. tree-sitter-latex finds all 28 `\section` commands and reports 289
error or missing nodes, all local. The corpus contains no verbatim environments.

A 232-byte input with a preamble macro, a verbatim block containing `{`, `$`, and
`%`, a `\verb|...|` span, and a theorem shows the limits:

- **MiTeX** tokenizes the verbatim content. The `{` inside it opens a group that
  swallows the rest of the file, with no error reported. `\verb` is not
  recognized. Macro expansion runs before parsing, so offsets refer to the
  expanded text: `\section`, at byte 28 of the file, is reported at offset 1.
- **tree-sitter-latex** handles the verbatim block and keeps exact offsets. The
  `\verb` span produces a local error node, after which parsing recovers.

Binary size: the compiled tree-sitter-latex parser tables are a 3.7 MB static
library, built from 44 MB of generated C. The gloss release executable is
17.5 MB. The resulting executable growth was not measured.

### Reproducing the measurements

`bench/latex` contains the harness. `corpus.py` downloads the pinned archives,
reads them in memory without extracting, and writes the inputs to a directory
outside the repository. It verifies every checksum below and fails on any
difference. `probe` is a standalone Cargo package with exact dependency versions
and a lock file. `run.py` runs the timings and prints medians. `scoping.tex` is
a pdfLaTeX reference for macro scoping and label placement; its comments state
the expected results.

```sh
python3 -I bench/latex/corpus.py <corpus-directory>
cargo build --release --manifest-path bench/latex/probe/Cargo.toml
python3 -I bench/latex/run.py <corpus-directory>
bench/latex/probe/target/release/latex-parser-probe structure <corpus-directory>/body.tex
bench/latex/probe/target/release/latex-parser-probe dump mitex <corpus-directory>/tricky.tex
bench/latex/probe/target/release/latex-parser-probe katex
```

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `1706.03762v7` source archive | — | `2e7a7d9ee2520d22eb23dae0cb148e167be02a30d6cb3f11b1c67723194339c6` |
| `2005.14165v4` source archive | — | `79c1afc60beccb0bd6d2f1362bb214e4bbda16e0dbc80c1b37d3b172049d5021` |
| `body.tex` | 374,049 | `a65b151ecedeb1eda523a50c128cb38fd070da6ec01b0de861f0176f6f5de3b9` |
| `body-x100.tex` | 37,404,900 | `da4f6442d1808f4701350b9ed2a37b005942779ebba6f7bd944db6cfed0f18a4` |
| `tricky.tex` | 232 | `c916fd1d542c3ce53973b0e997aa653c3b31379d4b552977ec193d0cc413ea17` |

The spike extends this harness to the candidate parsers' complete first passes
and the large synthetic documents described under [Validation](#validation).

## Suggested approach

### Components

| Piece | Location |
| --- | --- |
| LaTeX scanning, macro scoping, counters and labels, checkpoints, block and inline parsing, math preparation | New shared crate; name to be decided |
| Formula layout and rasterization | `katex-native`, unchanged |
| Macro map built from a context's bindings | Small `katex-native` helper, shared by both applications |
| PNG/JPEG/SVG figures | `document-graphics`, already shared |
| Root-document hints and `\input`/`\include` references | Text scanning in the shared crate; file access and path policy stay in each application |
| Layout, styling, caches, and revision publication | Each application |

`document-graphics` stays a rendering library without a document model, as its
scope requires. `katex-native` stays formula-only.

### Parser base

The proposal is a hand-written scanner modeled on MiTeX: keep its idea of a table
of command argument shapes, and borrow command data from MiTeX, unified-latex,
and tylax (keeping their license notices). Verbatim modes, local recovery, macro
scoping with source ranges, and checkpoints are written for this purpose. The
expectation is throughput comparable to MiTeX's tree builder with little effect on
executable size. That expectation is a hypothesis until the spike measures a
scanner that includes recovery, macros, and checkpoint construction.

tree-sitter-latex is the alternative. It recognizes more on day one and offers
incremental reparsing and highlighting, which favor live preview while editing.
Its costs are throughput and memory on large inputs, executable size, and
maintaining a generated parser. Macro scoping, numbering, and checkpoints must
still be written on top.

### Shared document model

The crate exposes a stream of events with source ranges, in the style of
`pulldown-cmark` but with its own types, since both applications already consume
streams of that shape. An illustrative outline:

```rust
pub enum Tag {
    Title, Abstract,
    Heading { level: u8, number: Option<Number> },
    Paragraph { indent: bool },
    List(ListKind), Item { label: Option<Label> },
    Quote, Code { language: Option<String> },
    Theorem { kind: String, number: Option<Number>, note: bool, style: TheoremStyle },
    Proof, Figure { number: Option<Number> }, Caption,
    Table { columns: Arc<[Column]> }, Row, Cell { span: u16, align: Align },
    Emphasis, Strong, Monospace, SmallCaps, SansSerif, Underline, Size(i8),
    Link { url: String }, Footnote { number: u32 },
    /// An unknown command or environment. Its delimiters stay visible as source.
    Unsupported { name: String },
}

pub enum Hint { Align(Align), VerticalSpace, PageBreak }

pub enum Event<'a> {
    Start(Tag), End(TagKind),
    Text(Cow<'a, str>),
    Math(Math<'a>),
    Reference { label: Cow<'a, str>, resolved: Option<Number> },
    Citation { keys: Vec<Cow<'a, str>>, resolved: Option<String> },
    Hint(Hint),
    LineBreak,
    /// Source shown as is: unsupported delimiters, over-limit regions.
    Raw,
}

pub struct Math<'a> {
    pub display: bool,
    /// TeX ready for KaTeX: labels removed, references resolved, rewrites
    /// applied, and explicit tags on numbered rows.
    pub prepared: Cow<'a, str>,
    pub context: MacroContext,
}
```

Each event carries the source range it came from. The model keeps LaTeX's
meaning rather than a Markdown shape: a GUI uses section numbers, indentation,
alignment, relative sizes, theorem styles, and table rules, and the terminal
ignores what it cannot show. `\input` and `\include` are reported as references
for the application to resolve. The crate has no file access and no KaTeX
dependency, which avoids coupling another crate to the katex-rs version.

### Input classification and fallbacks

Every command and environment falls into one of these explicit, tested classes:

1. **Mapped.** Converted to events, including presentation hints. `\centering`,
   `\raggedright`, and `center`/`flushleft`/`flushright` become alignment;
   `\noindent` sets `Paragraph { indent: false }`; `\vspace`, `\medskip`,
   `\newpage`, and `\clearpage` become spacing and page-break hints; size
   commands become `Size`. A consumer may ignore a hint, but the shared parser
   never discards it.
2. **Transparent wrappers.** An allowlist of commands whose argument renders
   without a marker because the wrapper has no visible meaning of its own, for
   example `\mbox`, `\textnormal`, `\textup`, and `\protect`. Each entry records
   why it is safe.
3. **No visible output.** An explicit list of commands that produce nothing in
   any consumer, such as `\label` (recorded for references), `\index`,
   `\phantomsection`, and `\relax`, plus preamble setup like `\usepackage` and
   `\geometry`.
4. **User definitions.** Commands and environments defined with `\newcommand`,
   `\newenvironment`, and related forms expand according to their definitions
   (see [Macro scoping](#macro-scoping)).
5. **Unknown.** Everything else. An unknown command becomes an `Unsupported`
   container: the command name and argument delimiters stay visible as `Raw`
   source, and arguments that parse as text render inside it so the content stays
   readable. An unknown environment keeps its `\begin{...}` and `\end{...}` lines
   visible around a parsed body. Environments known to contain non-text, such as
   `tikzpicture`, become `Raw` blocks. Each unknown construct adds a diagnostic
   with its name and position.

Diagnostics are bounded: the first occurrences keep positions, and later ones
only increment per-name counts. Hosts decide how to surface them.

### Macro scoping

A single document-wide macro map is not enough. A `\renewcommand` halfway through
a document must not change earlier formulas, and a definition inside a group must
not leak out of it. The crate follows TeX grouping:

- Groups are `{...}`, `\begingroup`/`\endgroup`, `\bgroup`/`\egroup`, every
  environment, and every formula.
- `\newcommand`, `\renewcommand`, `\providecommand`, `\DeclareMathOperator`,
  `\def`, `\let`, and `\newenvironment` are local assignments. `\gdef`, `\xdef`,
  `\global\def`, and `\global\let` are global assignments.
- The preamble has no special scope. Definitions at the outermost level, in the
  preamble or the body, last to the end of the document. Definitions inside an
  explicit group in the preamble are local to that group, as anywhere else.
- A definition inside a formula is local to that formula unless it is global,
  matching KaTeX's default `globalGroup: false`. A global definition inside a
  formula is visible to later content.
- Counter changes (`\setcounter`, `\addtocounter`, `\stepcounter`) are global in
  LaTeX, so counters are not scoped.

Each position has a macro context: the bindings visible there. Every `Math` event
carries its context, and text-mode expansion uses the same contexts.

- **Representation.** The crate follows TeX's save stack. The current bindings
  form an immutable map with structural sharing, so each assignment produces a
  new version and leaves earlier versions intact. Entering a group pushes a save
  frame. A local assignment records the name's previous binding in the innermost
  frame, once per frame, and then binds the new value. A global assignment binds
  the new value and marks the binding as global. Leaving a group restores each
  saved binding, except for names whose current binding was assigned globally
  since: those keep the global value. A context points to one map version, so
  contexts and checkpoints share state instead of copying it.
- **Global assignment example.** For `\def\x{a}{\gdef\x{b}}`, contexts before the
  group see `a`, contexts inside the group after `\gdef` see `b`, and contexts
  after the group still see `b`. The same rule holds when the group itself made a
  local definition first, as in `{\def\x{c}\gdef\x{b}}`. Earlier contexts keep
  `a`, because they point to an older map version.
- **Identity.** Each context has a 128-bit hash of its visible bindings: the sum
  of a strong hash of each binding's name and definition, updated as bindings
  change. Identical visible bindings give identical hashes however they were
  reached, across windows and reloads, so caches keyed on the hash survive both.
  Pointer identity would not survive a reload.
- **Building a macro map.** Constructing a context's KaTeX macro map is separate
  from rendering formulas. The map holds the context's visible bindings, one per
  name, built either directly from the crate's parsed definitions (name,
  parameter count, optional default, body) or by replaying those bindings through
  KaTeX with persistence enabled (`globalGroup: true`). Hosts cache built maps per
  context hash within a byte budget.
- **Rendering a formula.** Each formula renders with `globalGroup: false` on a
  copy of its context's map. Definitions inside the formula then stay local to
  it, and a `\gdef` inside the formula cannot modify the cached map. The crate
  has already applied such global definitions to later contexts.
- **Cache keys.** Every formula cache key includes the context hash alongside
  the prepared TeX and rendering parameters.
- **Incomplete contexts.** When a definition cannot be applied, because it
  exceeds a budget or is malformed, the binding it would have changed becomes
  unknown. Later content must not silently use the previous value: after a
  rejected `\renewcommand{\x}{b}`, a formula using `\x` must not render with an
  earlier `\x`. The context records the rejected names and is marked incomplete.
  A local rejection ends with its group, through the usual save-stack rule; a
  global one lasts to the end of the document. Content in an incomplete context
  falls back to source with a diagnostic. The first version applies this to every
  formula and text expansion in the context. A later refinement can limit it to
  content that uses a rejected name directly or through visible definitions.

Plastext's shared macro history is a useful starting point: it keeps formulas
that define macros in source order, bounds them, gives each position a context
identity, and falls back to source for every later formula once its limit is
exceeded. Its replay policy follows the Markdown convention, where any formula's
definitions apply to all later formulas through `globalGroup`. That is not
LaTeX's scoping model, so the LaTeX path uses the crate's contexts.

### Equation numbering and rewrites

The crate assigns every equation number. Prepared TeX carries an explicit
`\tag{n}` on each numbered row and uses unnumbered environments, so KaTeX's
automatic numbering is never involved. Numbers then do not depend on rendering
order, rewrites cannot change them, and references always agree with the display.
The rules follow the [amsmath manual](https://latex-project.org/help/documentation/amsldoc.pdf):

| Construct | Numbering |
| --- | --- |
| `equation` | One number, unless the body has `\notag` or `\nonumber`. `\tag{x}` replaces the number without advancing the counter. |
| `equation*`, `displaymath`, `\[...\]`, `$$...$$` | No number. `\tag{x}` still displays. |
| `split`, `aligned`, `gathered`, `multlined` inside `equation` | One number for the whole. |
| `align`, `gather`, `alignat`, `flalign`, `eqnarray` | One number per row (rows end at top-level `\\`), except rows with `\notag` or `\nonumber`. `\tag` replaces that row's number. A two-row `align` has two numbers. |
| Starred variants | No numbers. Rows may still carry `\tag`. |
| `multline` | One number for the whole environment. `\tag` applies to the whole. |
| `subequations` | Inner numbers 1a, 1b, …; a label on the outer environment refers to 1. |
| `\tag*{x}` | x without parentheses. |
| `\label` in a numbered or tagged equation or row | Binds to that number or tag. |
| `\label` in an unnumbered row of an amsmath environment | Stays pending and binds to the next numbered or tagged row of the same environment. If no such row follows, the label is undefined. |
| `\label` in an unnumbered environment without a tag, such as `equation*` | Binds to the current label: the most recent numbered item still in scope, such as the section. |
| `\setcounter{equation}`, `\addtocounter{equation}` | Applied in source order. |
| `\numberwithin{equation}{section}` | Resets at each section and formats numbers as section.number. |
| Redefined `\theequation` | Out of scope: numbers use the default format, with a diagnostic. |

`\ref` produces the bare number or tag, and `\eqref` the same in parentheses.

Label placement in display math depends on amsmath. In an `align` whose first row
has `\notag` and a label, followed by a numbered row, the label resolves to the
second row's number, not to the enclosing section: with section 7 and the next
equation 41, `\ref` gives 41. A label on a final `\notag` row is never defined.
A label in `equation*` resolves to the section, 7, even after an `align` that
numbered equations. LaTeX sets the current label locally, so numbers made inside
an environment do not outlast it. The crate tracks the current label as a scoped
binding, with the same save-stack rules as macros.

These cases were checked with pdfLaTeX from TeX Live 2024 (LaTeX 2023-11-01,
amsmath 2.17o), using
[`bench/latex/scoping.tex`](../bench/latex/scoping.tex). The crate reproduces
the behavior in the table. Other placements are not guessed: they leave
references unresolved, shown as the key in brackets, with an
unsupported-placement diagnostic. Examples are several labels pending for one
row, and documents that use `eqnarray` without amsmath, where the kernel's rules
apply instead.

Rewrites change the environment only after numbering is decided, and the probe
confirmed each target renders:

| Source | Prepared for KaTeX |
| --- | --- |
| `equation`, `align`, `gather`, `alignat` | The starred form, with `\tag{n}` on each numbered row. |
| `multline` | `gathered` inside `equation*`, with one `\tag{n}`. Its first-line-left, last-line-right alignment becomes centered. |
| `eqnarray` | `align*`, with `&op&` reduced to `&op` and `\tag{n}` on each numbered row. |
| `flalign` | `align*`, with `\tag{n}` on each numbered row. Alignment to the margins is lost. |

Whether `katex-native` places per-row tags against the right rows is checked
visually in the spike.

### Paging and checkpoints

The first pass is a linear, cancellable scan. It produces the binding store,
labels and their numbers, the outline, diagnostics, and checkpoints. The main
parser starts at any checkpoint: gloss streams once from the start, and a
virtualized GUI parses only the windows it shows.

- **Compact checkpoint records.** A checkpoint holds the source offset, the
  lexer mode (normal, or verbatim with its end delimiter), the macro context as a
  version number in the binding store, the open save frames, the
  open-environment stack, and counter values. The environment stack and save
  frames are bounded by the nesting limit and stored inline. Each environment
  frame holds what rendering needs, such as a list's item number or a table's
  column specification. Records reference only the binding store, which stays
  resident. The definition budget counts every assignment, including
  redefinitions, so it also bounds the store; exceeding it makes later contexts
  incomplete rather than growing the store. A record is therefore self-contained
  apart from that store and can be moved out of memory. The target is under 256
  bytes per record at typical nesting.
- **Fixed spacing.** Checkpoints are placed at the first safe boundary after each
  32 KiB of source, not at every block. Spacing never grows with document size,
  so checkpoints are never thinned.
- **Safe boundaries inside large environments.** A safe boundary is a point
  whose parser state the checkpoint fully describes: a paragraph break at any
  environment depth, an `\item`, the end of a table row, a line end inside
  verbatim, and the end of display math. A 10 MB `itemize`, `verbatim`, or
  `tabular` therefore still gets a checkpoint every 32 KiB or so.
- **Replay budget.** A window parse may scan at most a fixed number of bytes,
  the replay budget, between its starting checkpoint and the end of the window.
  With fixed spacing, an ordinary window stays well inside it. A construct with
  no safe boundary that would exceed it, such as one enormous formula, paragraph,
  or brace group, is emitted as `Raw` source split at line boundaries, with a
  diagnostic, and the first pass places the next checkpoint after it. The cost of
  any window parse is bounded by the replay budget, independent of document size.
- **Bounded resident storage.** Records stay in memory up to a fixed cap. Beyond
  it, they are appended to an anonymous temporary file, as gloss's pager and
  Plastext's PDF snapshots already do, and read back by position. The file has a
  disk quota. If the quota is reached, indexing stops at the last checkpoint: the
  remainder of the document is shown as `Raw` source in replay-budget windows,
  with a diagnostic. gloss's 64 MiB input limit needs about 2,048 records, well
  within any resident cap; spilling serves inputs without that limit. The spike
  sets the spacing, replay budget, resident cap, and quota from measurements.
- **Early display.** A host can show the first window before the first pass
  completes. Counters and macros for that window are known once the text before
  it has been scanned; only forward references stay unresolved until the pass
  finishes.

Paging is part of the spike, not a later step, because the checkpoint model
constrains the parser's state representation.

### Cancellation and revisions

Both applications reload documents, and Plastext previews unsaved edits.

- **Cooperative cancellation.** The first pass, window parses, and macro
  expansion take a cancellation flag (an `AtomicBool`, as gloss's pager worker
  and `document-graphics` already use). They check it at least every 64 KiB of
  input and every fixed number of expansion steps, and return a cancelled result
  without publishing partial state. The spike measures cancellation latency.
- **Revisions.** An index records an opaque revision supplied by the host. Window
  parses take the index and the matching source snapshot, and their results carry
  the same revision. Hosts publish only results whose revision matches the
  current source.
- **Last successful preview.** Hosts keep the last successful preview visible
  while a new revision is indexed, and after errors. gloss's pager already keeps
  the last complete page during a watched reload; the LaTeX path polls the same
  cancellation flag that the pager sets.
- **Reuse after edits.** Checkpoints before the first changed byte remain valid,
  so a host can resume the first pass from the last checkpoint before an edit.
  This is an optimization for a later increment.

### Resource limits and failure propagation

Limits apply to macro expansion depth, steps, and output; group and environment
nesting; definitions and their total size; labels; diagnostics; checkpoint
storage and replay; and block size. They follow the style of the existing
[Markdown extension limits](markdown-extensions.md#backslash-math).

Exceeding a limit keeps the affected region as source, with a diagnostic. A
failure must not change later content silently, so state derived from a failed
or skipped region is marked uncertain, and content that depends on it falls back:

- **Macros.** A rejected or skipped definition makes later contexts incomplete,
  as described under [Macro scoping](#macro-scoping). Definitions inside a region
  emitted as `Raw` count as skipped.
- **Numbering.** If a region that could change a counter is emitted as `Raw`,
  such as display math or a sectioning command inside it, later numbers from that
  counter are uncertain. They are shown with a diagnostic marker instead of a
  possibly wrong number, until a reset, such as `\setcounter` or a section reset
  under `\numberwithin`, makes them known again.
- **Labels.** A label dropped by the label budget, or defined inside a skipped
  region, leaves its references unresolved, shown as the key in brackets.

Expansion that exceeds its budget falls back to source for that invocation. If
the unexpanded remainder could define macros or change counters, the same
marking applies; otherwise later content is unaffected. The rest of the document
continues to render in every case.

### Gloss integration

- Add `Format::Latex`, detected for `.tex`, `.latex`, and `.ltx`, and selectable
  with `--input-format latex`. `--input-format code` keeps LaTeX literal.
- Add a LaTeX document kind that runs the first pass, then maps shared events onto
  gloss's events. Headings, lists, quotes, code, links, and footnotes use existing
  events, which keeps the appearance consistent with Markdown.
- Exact source output returns the original `.tex`. Plain export contains readable
  prose with the original math delimiters, as for Markdown.

Expected interface changes:

- **Formula context.** `pulldown-cmark`'s math events carry only TeX. LaTeX
  formulas also need prepared TeX and a macro context, so gloss's event wrapper
  and the layout's math runs and display blocks gain an optional context and
  prepared text. `MathEngine` gets per-context macro maps and adds the context
  hash to its cache key.
- **Numbering.** LaTeX formulas carry explicit tags, so `MathEngine` renders them
  without advancing its automatic equation counter.
- **Tables.** `p{width}` columns, `\multicolumn` spans, and rules do not fit
  `pulldown-cmark`'s per-column alignments. The first version approximates them:
  a span puts its content in the first cell and leaves the others empty, and
  widths are ignored. A later change can extend the table model.
- **Fallbacks.** `Raw` and `Unsupported` source needs a terminal style, likely the
  existing math-source or code styling.

| LaTeX | Terminal presentation |
| --- | --- |
| `\maketitle` with `\title`, `\author`, `\date` | Level-1 heading, then author and date lines |
| `\section` … `\subparagraph`, starred forms | Headings of the matching level, with section numbers |
| `abstract`, `quote`, `quotation` | Quotes; the abstract gets a label |
| `\emph`, `\textit`, `\textbf`, `\texttt`, `{\bf ...}` | Italic, bold, inline code |
| `itemize`, `enumerate`, `description` | Lists; description terms in bold |
| `verbatim`, `lstlisting`, `minted`, `\verb` | Code blocks, highlighted when the language is supported; inline code |
| `$...$`, `\(...\)`; `$$...$$`, `\[...\]`, `equation`, `align`, `gather` | Inline and display math through the existing math path |
| `\href`, `\url`, `\footnote` | Links, footnote references and definitions |
| `tabular` | Tables, with column alignment from the column specification |
| `\includegraphics` with `\caption` | Image and numbered caption; unsupported formats keep a reference |
| `theorem`, `lemma`, `definition`, `proof` | Bold "Theorem 2 (Name)." followed by the body; "Proof." and an end mark |
| `\ref`, `\eqref`, `\cite` | Resolved numbers, or the key in brackets |
| ``` ``...'' ```, `--`, `---`, `~`, `\'e`, `\%` | Unicode text |
| Alignment, spacing, and page-break hints | Ignored |
| Unknown commands and environments | Their source, around readable arguments or bodies |

### GUI integration

Plastext's Markdown preview is already virtualized: it indexes 64 KiB pages,
records the macro context in effect at any offset, and parses only visible pages.
The shared crate's checkpoints and contexts fit that structure. A first adapter
can reuse the preview's virtualized list, math rendering, find, and selection. A
LaTeX-like presentation then adds body text in KaTeX_Main (derived from Computer
Modern and already bundled by `katex-native`), first-line paragraph indents,
alignment hints, numbered headings, a centered title block, theorem and proof
styles, and numbered captions. Justification and hyphenation depend on the GUI
text system and are outside this design. PDF compilation remains available as
the exact mode.

## Delivery

1. **Spike.** Build the same slice on a hand-written scanner and on
   tree-sitter-latex, and choose the base from the results. The slice covers:
   - sections, paragraphs, emphasis, lists, math, verbatim, `\verb`, and labels;
   - checkpoints with fixed spacing, safe boundaries inside large environments,
     the replay budget, spilling beyond the resident cap, and the fallback at the
     disk quota;
   - cancellation of the first pass and of expansion;
   - scoped macro contexts, including global assignments inside groups, content
     hashes, and incomplete contexts;
   - the numbering rules, label placement, and rewrites, as fixtures;
   - the fallback classes, failure propagation, and diagnostics.

   Compare complete first-pass throughput, checkpoint memory, bytes replayed and
   latency for windows near the end of large documents, cancellation latency,
   code size, executable size, and output on the corpus.
2. **Shared crate core and gloss adapter.** Detection and `--input-format latex`,
   the preamble and title, sectioning, inline styles, lists, quotes, code, links,
   footnotes, math, and fallbacks. Add a `tests/fixtures/latex.tex` fixture.
3. **Structure.** Cross-references for sections, theorems, figures, and tables;
   `tabular`; figures; theorems; and text-mode user macros. Then the Plastext
   adapter, with the fast preview alongside PDF compilation.
4. **Later.** Resuming the first pass after edits, `\input`/`\include` (which
   affects `--watch`), bibliographies (the
   [`biblatex`](https://crates.io/crates/biblatex) crate is a candidate), and
   broader package coverage. TikZ stays as source.

### Spike outcome

The hand-written scanner was built first, with recovery, scoped macros,
amsmath numbering, the fallback classes, cancellation, and checkpoints with
fixed spacing and a cap. Its complete first pass, measured with the crate's
`throughput` example on the corpus above (medians, same machine), already
outperforms tree-sitter-latex's syntax-only parse by an order of magnitude:

| Input | First pass | Events | Peak RSS |
| --- | ---: | ---: | ---: |
| 374 KB | 3.0 ms (125 MB/s) | 3.9 ms | — |
| 37 MB | 236 ms (159 MB/s) | 305 ms | 41 MiB |

Peak RSS includes the 36 MiB input. tree-sitter-latex needed 3.54 s and
1,273 MiB to parse the same 37 MB without any semantics, so the tree-sitter half
of the spike was not built: it cannot meet the throughput and memory goals.
Window equivalence holds for every checkpoint of a synthetic document with
checkpoints inside lists, verbatim blocks, and tables, including past the cap.

In gloss, plain export of a 10 MiB Markdown document is unchanged (213 ms
against 215 ms on the parent commit, medians of seven alternating runs), and
the release executable grows by 318,304 bytes (1.8%). Plain export of the 37 MB
LaTeX input takes 687 ms with a 45 MiB peak.

Not yet built: spilling checkpoint records to disk past a resident cap,
splitting paragraphs or formulas larger than the replay budget, corpus scoring
against reference PDFs, and the Plastext adapter.

Rough sizes, including tests: 5,000–6,000 lines for the shared crate, about 600
lines plus documentation for the gloss adapter and its interface changes, and
about 500 lines for a Plastext adapter that reuses its preview machinery. The
GUI styling work is not sized.

## Validation

- **Corpus scoring.** Reuse `katex-native`'s method: take public paper sources,
  build reference PDFs once with pdfLaTeX, and measure the share of the reference
  text retained, the share of display formulas that render, and agreement of
  headings and numbering.
- **Checkpoint equivalence.** For every checkpoint in the corpus and in
  synthetic documents, a window parse from that checkpoint produces the same
  events as the corresponding part of a sequential parse.
- **Large documents.** Synthetic inputs with a 10 MB `itemize`, a 10 MB
  `verbatim`, a 5 MB `tabular`, a 1 MB paragraph, deep nesting, and many
  redefinitions. Tests open a window directly near the end and check latency,
  peak memory, and the macro context and counters there.
- **Beyond the checkpoint cap.** The same tests run with small resident caps and
  disk quotas, so that records spill and the quota is reached. They assert that
  bytes scanned per window stay within the replay budget whatever the document
  size, that spilled records give the same results as resident ones, and that
  content past the quota is shown as source with a diagnostic.
- **Macro scoping.** Fixtures assert which formulas see which definitions for:
  - redefinition halfway through a document;
  - group-local and environment-local definitions;
  - definitions inside an explicit group in the preamble, which stay local;
  - `\def\x{a}{\gdef\x{b}}` and `{\def\x{c}\gdef\x{b}}`, where later content sees
    `b` and earlier content `a`;
  - global definitions inside formulas;
  - a redefinition rejected by the definition budget, after which formulas using
    the macro fall back to source.
- **Numbering.** Fixtures for every row of the numbering table, including
  references to each case. Label placement fixtures cover a label on a numbered
  row; a label on a `\notag` row followed by a numbered row, which resolves to the
  later row (section 7, equation 41 gives 41); a label on a final `\notag` row,
  which is undefined; a label in `equation*` after an `align`, which gives the
  section number; several pending labels; and `eqnarray` without amsmath. The
  last two expect unresolved references and diagnostics. Expected values come
  from `bench/latex/scoping.tex` compiled with pdfLaTeX.
- **Failure propagation.** A display formula inside a region emitted as `Raw`
  makes later equation numbers uncertain until a counter reset; a label inside it
  leaves its references unresolved.
- **Fallbacks.** Unknown commands and environments keep their names visible, and
  diagnostics report them.
- **Cancellation.** Cancellation latency during the first pass, during a window
  parse, and during runaway expansion.
- **Gloss.** Markdown output stays byte-identical, and unchanged paths are
  measured against the [fixed baseline](performance-baseline.md). Record
  executable size and dependency changes. Integration tests cover detection,
  exact source and plain exports, control-character filtering, formula fallbacks,
  numbering, and unknown-input preservation.

## Open questions

- The shared crate's name and repository. Avoid `latex-native`: unlike
  `katex-native`, it suggests a TeX engine.
- Whether gloss resolves `\input` and `\include` relative to the document, and
  how that interacts with `--watch`, which currently watches one file.
- How gloss surfaces diagnostics: always, as a summary on stderr, or only on
  request.
- Plastext's roadmap currently keeps document parsing within each application. A
  shared LaTeX crate changes that decision there.
