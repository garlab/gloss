# LaTeX documents

`.tex`, `.latex`, and `.ltx` files, or any input with `--input-format latex`,
are read natively by [`latex-reader`](https://crates.io/crates/latex-reader): no
TeX installation, PDF, or intermediate Markdown is involved. The
[README](../README.md#latex-documents) lists what is supported. This page
describes how gloss presents LaTeX, its limits, and how it is validated.

## Presentation

The `src/document/latex.rs` adapter maps the crate's events onto the CommonMark
events gloss already renders, so equivalent constructs look like Markdown and
layout, exports, and the pager are shared.

| LaTeX | Terminal presentation |
| --- | --- |
| `\maketitle` | Level-1 title, author and date lines; sections start one level lower |
| `\section` … `\subparagraph` | Headings, with section numbers as text |
| `abstract` | A quote with a bold "Abstract" line |
| `\emph`, `\textbf`, `\texttt` and declarations | Italic, bold, inline code |
| `itemize`, `enumerate`, `description` | Lists; a first paragraph stays tight |
| Theorems and proofs | Bold "Theorem 1.1 (Note)." before the body; proofs end with ∎ |
| `tabular` | Tables; a first row followed by a rule is the header; spans add empty cells |
| `\footnote` | Footnote references, with definitions at the end |
| `\ref`, `\eqref`, `\cite` | Resolved numbers, or the key in brackets |
| `verbatim`, listings, `\verb` | Code blocks and inline code, highlighted when the language is supported |
| `thebibliography` | A "References" heading and a list |
| Alignment, spacing, and page-break hints | Ignored |
| Unknown commands and environments | Their source as inline code or a code block, around readable arguments |

Plain export (`--output text`) contains the prose, including included files,
with each formula's original source. Source export returns the main file's
bytes unchanged.

## Formulas

Each formula is rendered from the crate's prepared TeX, with the macros visible
where it starts. `MathEngine` builds a KaTeX macro map per macro context, keeping
at most eight, and renders each formula with `globalGroup` off on a copy, so
definitions inside a formula stay local. Equation numbers come from the prepared
TeX as explicit tags; KaTeX's numbering is not used. The math cache key includes
the context hash. A formula whose context is incomplete, because an earlier
definition could not be applied, falls back to its source with a warning.

## Included files and figures

Included files resolve against the document's directory, or `--base-dir`, as
LaTeX resolves them from the main file. `src/input.rs` reads them: regular files
only, opened without blocking, invalid UTF-8 replaced, and at most 64 MiB of
expanded text, 15 nested files, and 256 reads. A file that cannot be read keeps
its command visible as source. The document keeps the main file for source
export and the expanded text for events and plain export.

`--watch` polls the files the last successful render read, up to 63 besides the
named file, and forces a render when one changes. Files that did not exist at
that render are not polled.

`\includegraphics` uses the shared image loader. A name without an extension
tries `.png`, `.jpg`, `.jpeg`, and `.svg`, also in `\graphicspath` directories;
only existing candidates are opened. PDF and EPS figures keep their reference.

## Limits

The crate's limits apply: definitions, nesting, expansion, formula size, labels,
and diagnostics are bounded, and exceeded regions stay visible as source. gloss
adds its text input limit and image, math, and output budgets. Diagnostics are
not shown; unknown input is visible in the output instead.

Not supported: PDF and EPS figures, TikZ (shown as source), BibTeX databases
without a `.bbl` file, and anything that needs running TeX, such as package
code or exact line and page breaking.

## Validation

Integration tests cover detection, plain and source exports, equivalence of
headings and lists with Markdown, numbered formulas with document macros, macro
scopes, and included files with listings, `.bbl` files, and missing files.
`tests/pager_smoke.py` checks that editing an included file reloads a watched
document. `tests/fixtures/latex.tex` exercises the common constructs.

The reader itself is tested in its own repository, against pdfLaTeX for
scoping and label placement, with a corpus of public arXiv papers.

## Measurements

Release builds on an Apple M2 Pro with Rust 1.99. Adding LaTeX left plain export
of a 10 MiB Markdown document unchanged (213 ms against 215 ms, medians of seven
alternating runs) and grew the executable by 318,304 bytes (1.8%). Plain export
of a 37 MB LaTeX input takes 687 ms with a 45 MiB peak.

With the sources of the two corpus papers (arXiv 1706.03762v7 and 2005.14165v4)
extracted, their main files include 7 and 97 files and render to a 100×40 PTY
with 10×20-pixel cells in 54 ms and 184 ms. For the 474 formulas of the joined
corpus at 18 pixels per em, KaTeX layout takes 4.4 ms, rasterization 25 ms, and
compression for Kitty graphics 6 ms, so rasterization dominates formula-dense
documents. Pager timings are in [Pager](pager.md#first-screen-preview).
