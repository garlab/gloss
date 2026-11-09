# gloss

[crates.io](https://crates.io/crates/gloss-cli) ·
[releases](https://github.com/garlab/gloss/releases) ·
[Homebrew tap](https://github.com/garlab/homebrew-tap)

A terminal viewer for Markdown, LaTeX, math, diagrams, images, and code.
Equations, Mermaid diagrams, Go boards, and local images render as graphics in
Ghostty and Kitty, while prose stays selectable terminal text; other terminals
and pipes get readable text with the original source. No browser, Node.js, or
TeX installation is needed.

```sh
gloss notes.md              # print into the terminal's scrollback
gloss --pager paper.tex     # read interactively
gloss --watch notes.md      # re-render whenever the file is saved
cat notes.md | gloss -      # read stdin
```

## Install

With Homebrew:

```sh
brew install garlab/tap/gloss
```

Bottles cover Apple Silicon on macOS 26 or newer and Linux x86-64; other
platforms build from source. The tap follows stable releases automatically.

With Cargo (Rust 1.90 or newer and a C compiler):

```sh
cargo install gloss-cli --locked
```

The Cargo package is `gloss-cli`; the executable is `gloss`. The pager and
`--watch` need a Unix terminal, including macOS; Windows uses direct output.

## What gloss renders

### Markdown and math

- Headings, emphasis, lists, quotes, code, links, task lists, strikethrough,
  footnotes, and aligned, wrapped tables.
- `$...$` / `\(...\)` inline math, `$$...$$` / `\[...\]` display math, and fenced
  `math` blocks, with equation tags and automatic numbering.
- GitHub alerts, inert front matter, expanded details, and terminal hyperlinks.
- Local PNG, JPEG, and SVG images, from Markdown or HTML `<img>`, with
  selectable captions.
- Mermaid diagrams and SGF Go boards in fenced blocks.
- Syntax colors for Rust, Python, JavaScript/JSX, TypeScript/TSX, JSON,
  Bash/shell, Go, TOML, YAML, and Dockerfile/Containerfile.

Math follows the KaTeX subset implemented by
[`katex-native`](https://crates.io/crates/katex-native). A formula that cannot
render falls back to its source without interrupting the document. Other HTML is
shown as source and never executed, and remote images are never fetched.

### LaTeX documents

`.tex`, `.latex`, and `.ltx` files are read natively by
[`latex-reader`](https://crates.io/crates/latex-reader) for a fast, approximate
preview; no TeX installation or PDF is involved. Equivalent constructs look like
Markdown: `\section` renders as a heading, `itemize` as a list, and math through
the same renderer.

- `\maketitle` with `\title`, `\author`, and `\date`; `abstract`; numbered
  sections by document class, including chapters and `\appendix`.
- Emphasis, bold, monospace, links, footnotes, lists, quotes, and `tabular`
  tables with column alignment; a first row followed by a rule becomes the
  header.
- Theorems from `\newtheorem`, rendered as a bold "Theorem 2 (Name)." before the
  body; proofs end with ∎.
- `\ref`, `\eqref`, and `\cite` resolved against labels and the bibliography.
- Math numbered as amsmath numbers it, row by row, with `\notag` and `\tag`.
- Macros from `\newcommand`, `\def`, `\let`, and `\DeclareMathOperator`, with
  TeX's scoping: a redefinition affects only later formulas.
- `verbatim`, `lstlisting`, `minted`, and `\verb` stay literal; `comment` and
  `\iffalse` blocks are hidden.
- Included files, read relative to the document's directory (or `--base-dir`):
  `\input`, `\include`, `\subfile`, and `\import`; `\lstinputlisting`,
  `\VerbatimInput`, and `\inputminted` as code; `\bibliography` from the
  document's `.bbl` file; and `\includegraphics` for PNG, JPEG, and SVG, with or
  without an extension and searching `\graphicspath`.

Unknown commands and environments stay visible as source, around their readable
arguments; TikZ pictures and PDF figures keep their source or reference.
`--watch` also reloads when an included file changes. See
[LaTeX documents](docs/latex.md) for details and limits.

### Code, diagrams, and images

Named files are detected by extension; anything else, and stdin, is Markdown.

| Input | Extensions |
| --- | --- |
| LaTeX | `.tex`, `.latex`, `.ltx` |
| Mermaid | `.mmd`, `.mermaid` |
| SGF Go records | `.sgf` |
| Images | `.png`, `.jpg`, `.jpeg`, `.svg` |
| YAML, TOML, JSON | `.yaml`, `.yml`, `.toml`, `.json`, and `Cargo.lock` |
| Rust, Python, Go | `.rs`, `.py`, `.pyi`, `.pyw`, `.go` |
| JavaScript, TypeScript | `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.mts`, `.cts`, `.tsx` |
| Bash / shell | `.sh`, `.bash` |
| Dockerfile | `Dockerfile`, `Dockerfile.dev`, `build.Dockerfile`, and `Containerfile` names |

```sh
gloss diagram.mmd
gloss picture.png
gloss --sgf-move 42 record.sgf
gloss config.yaml
gloss --input-format code settings.conf
cat config.yaml | gloss --language yaml -
```

`--input-format markdown|latex|code|mermaid|sgf|image` overrides detection, and
`--language` selects code mode with a syntax language, also for stdin and files
without an extension. Code mode keeps source literal, including Mermaid and SGF.
Extensions are case insensitive. Text inputs must be UTF-8; images need a named
local file.

Mermaid covers flowcharts, sequence, class and state diagrams, ER tables, pie
charts, mind maps, timelines, user journeys, quadrant charts, and
[Gantt schedules](docs/gantt.md), natively and without full Mermaid.js
compatibility. SGF boards show the final position of the first game's main line
by default; `--sgf-game` and `--sgf-move`, or fence attributes such as
`sgf game=2 move=12`, select others. Unsupported or over-limit content keeps its
source. See [Mermaid](docs/mermaid.md), [SGF](docs/sgf.md), and
[local images](docs/images.md).

## Terminal output

Graphics are used in sessions identified as Ghostty or Kitty by `TERM` or
`TERM_PROGRAM`, without probing the terminal. Other terminals, tmux, screen,
Zellij, `TERM=dumb`, and very small windows keep equations as selectable TeX,
and diagrams, boards, and images as code or references. Each kind of graphics
can be forced or disabled independently.

The reading column is at most 80 cells wide, with two-column margins that shrink
in narrow windows. Equations follow the terminal's cell height and shrink to
fit; images and diagrams fit the column without enlargement, and tall diagrams
continue down the page. Tables wrap within the column, and narrow windows show
numbered cells instead. The theme follows `COLORFGBG` when available and is dark
otherwise. A nonempty `NO_COLOR` disables text styling but not images.

| Option | Behavior |
| --- | --- |
| `--math auto\|tex\|kitty` | Render equations as graphics, keep them as source TeX, or force graphics. |
| `--show-math-source` | Print selectable TeX beneath rendered equations. |
| `--images auto\|off\|kitty` | Local images; `off` keeps references and skips reading image files. |
| `--base-dir <directory>` | Resolve local image and LaTeX file paths from another directory. |
| `--mermaid auto\|off\|kitty` | Mermaid diagrams; `off` keeps the code. |
| `--show-mermaid-source` | Print selectable code beneath rendered diagrams. |
| `--sgf auto\|off\|kitty` | Go boards, independently of other graphics. |
| `--sgf-game <number>` / `--sgf-move <number>` | Select a one-based game, or a main-line move (`0` is the root). |
| `--show-sgf-source` | Print selectable SGF beneath rendered boards. |
| `-w, --width <columns>` | Maximum content width, excluding margins; `0` uses the available space. |
| `--margin <columns>` | Horizontal margin, default `2`. |
| `--theme auto\|dark\|light`, `--light` | Select the palette. |
| `--line-numbers auto\|on\|off` | Code line numbers; `auto` numbers standalone code only. |
| `--no-highlight` | Disable syntax colors, keeping the rest of the theme. |
| `--links auto\|off\|always` | Terminal hyperlinks; visible targets are kept either way. |
| `--cell-size <width>x<height>` | Override the terminal cell size in pixels. |
| `--font-size <pixels>` | Override the equation font size. |

See [terminal discovery](docs/terminal-detection.md),
[syntax highlighting](docs/syntax-highlighting.md),
[tables](docs/tables.md), and [Markdown extensions](docs/markdown-extensions.md)
for details and fallbacks.

## Pager and live updates

```sh
gloss --pager notes.md
gloss --watch notes.md
cat notes.md | gloss --pager -
```

`-p` / `--pager` opens an alternate screen and restores the previous one on
exit. The first screen appears as soon as it is rendered; the footer shows `+`
and `rendering` until the rest of the document follows. Text reflows when the
window changes size, and equations, images, and diagrams scroll with it.
Keyboard input comes from the terminal, so piped documents work too.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows, Enter | Scroll down/up one row; Enter scrolls down. |
| Space / `b`, Page Down / Page Up | Move a page, keeping one row of overlap. |
| `d` / `u`, Ctrl-D / Ctrl-U | Move half a page. |
| `g` / `G`, Home / End | Go to the beginning/end. |
| `/` / `?`, Enter | Search forward/backward for literal, case-sensitive rendered text. |
| `n` / `N` | Repeat/reverse the last search, wrapping through the document. |
| `r` | Reload the file and its local images; rerender piped input. |
| `w` / `h` | Cycle rendering warnings / show key hints. |
| `q`, Escape, Ctrl-C | Quit; Escape first cancels an open search prompt. |
| Ctrl-Z | Restore the terminal and suspend; `fg` resumes. |

`--watch` implies the pager and needs a named file. It reloads after in-place
and atomic saves, keeps the last good page through temporary errors, and keeps
your position; staying at the end follows appended content. The document and
LaTeX included files are watched; press `r` after changing an image alone.

Search covers terminal text, not letters inside equations or diagrams. Mouse
tracking stays off, so native text selection works. `--pager` is ignored for
pipes, `--output text`, `--output source`, and `TERM=dumb`. Inputs are limited
to 64 MiB. See [pager](docs/pager.md) for limits and measurements.

## Copying and pipes

```sh
gloss --output text notes.md > notes.txt
gloss --output source notes.md | pbcopy  # macOS
```

Pipes and `--output text` receive readable text without styles or graphics:
equations as source TeX, images as references, and diagrams as code.
`--output source` returns the original file byte for byte, without parsing.

Terminal selection copies prose. Selecting an equation image does not recover
its TeX: use `--show-math-source` or `--math tex` when copying math, and
`--show-mermaid-source` or `--show-sgf-source` for diagrams. gloss never changes
the clipboard itself.

## Documentation

The [documentation index](docs/README.md) lists the feature references: syntax
highlighting, tables, images, Mermaid, SGF, Markdown extensions, LaTeX
documents, the pager, and terminal discovery, with their limits and
measurements.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for building from a checkout, the checks
to run, and where design decisions are recorded.

## License

MIT; see [LICENSE](LICENSE). `katex-native` is MIT, and its bundled KaTeX fonts use
the SIL Open Font License 1.1; see the [font license](https://github.com/garlab/katex-native/blob/main/fonts/OFL.txt).
