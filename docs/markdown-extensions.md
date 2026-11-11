# Markdown extensions

These extensions preserve exact source output. Plain export includes readable
content and original equation delimiters; terminal decoration never enters it.
See `tests/fixtures/extensions.md` for a compact example.

## Alerts

GitHub's `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, and `[!CAUTION]` quote
markers become labeled quotes. Labels remain visible without color and in plain
export. Recognition uses the CommonMark parser's GFM extension; ordinary quotes
keep their existing behavior.

## Front matter

A leading `---` or `+++` line followed by a matching closing line is shown as an
inert code panel, including its original delimiters. YAML-style blocks also
accept `...` as the closing delimiter. Content must be nonempty, and the complete
block must fit within 16 KiB. Missing delimiters and oversized blocks remain
ordinary Markdown.

This recognizes a delimiter convention, not a YAML or TOML schema. It does not
interpret tags, expand aliases, load files, apply viewer settings, or execute
metadata. Even invalid metadata remains visible. Exact source export preserves
line endings and all bytes.

## Details and summary

Balanced, standalone `<details>` or `<details open>` and `</details>` lines are
expanded. A single-line `<summary>text</summary>` becomes a bold paragraph, with
inline Markdown and math supported. Body content uses ordinary Markdown, and
nested sections remain expanded. Details tags may have up to three leading
spaces; wrappers inside lists or quotes and arbitrary HTML attributes are not
part of this initial subset. Summaries containing `<` or `>` characters stay literal.

Each section is limited to 256 KiB and 16 nesting levels, with a total budget of
16,384 wrapper edits. Unclosed or over-limit wrappers retain their source.
Code spans, fenced/indented code, comments, and other HTML blocks are excluded.
This is not an HTML renderer; it never executes scripts or retrieves resources.

## HTML markup

Common README HTML renders like the equivalent Markdown. `<h1>` to `<h6>` become
headings, `<p>` and `<div>` separate paragraphs, `<b>`/`<strong>`, `<i>`/`<em>`,
and `<s>`/`<del>` become strong, emphasis, and strikethrough, `<code>` a code
span, `<a href>` a link, `<br>` a line break, and `<hr>` a rule. An `<img>`
renders like a Markdown image: a local file is drawn under the
[local image](images.md) rules; any other source, including a remote URL, keeps
its `[image: alt] (src)` reference. Nothing is fetched. This covers HTML blocks,
such as a centered logo, title, and tagline, and runs of adjacent inline tags,
such as a linked badge:

```html
<p align="center">
  <a href="https://example.com"><img src="docs/logo.png" alt="Logo" width="200"></a>
</p>

<h1 align="center">Project</h1>

<p align="center">
  <b>A one-line description.</b><br>
  More detail on a second line.
</p>
```

A fragment converts only when it shows an image, text, or a rule, and contains
only these elements, `<picture>`, `<source>`, comments, and text. Comments are
hidden. Text follows HTML whitespace rules, with entities decoded, including
inside `<code>`; it is not Markdown. Elements other than `<code>` that are left
open close at the end of the fragment. Exports match the equivalent Markdown, and exact source export is
unchanged.

Any other element, a heading or rule inside a link or style, a tag inside
`<code>` or one left open, a missing or empty `src`, or malformed markup leaves
the whole fragment as source. Inside a Markdown paragraph, only links, styles,
pictures, and images convert, and they must close within their run of tags, so
`Some <b>bold</b> text` and `one<br>two` keep their source. `width`,
`height`, and `align` are ignored: images keep the usual fitting, and headings and
paragraphs keep the usual alignment. A `<picture>` shows its `<img>`; light and
dark `<source>` alternatives are not selected. A wrapper on its own, such as a
`<div align="center">` line around Markdown, still shows as source.

Each fragment is limited to 256 KiB, 32,768 tokens, 16 KiB per attribute value,
and 128 nested wrappers; larger fragments keep their source. Fragments without a
supported start tag skip tokenization, and tokenization stops at the first
unsupported element.

When image fragments were first converted, output on the benchmark corpora, which
contain no HTML, stayed byte-identical; plain export of the 10 MiB mixed document
measured 98.2 ms against 95.4 ms before (medians of five alternating seven-sample
runs, Apple M2 Pro, Rust 1.99). A 2.2 MB document with 12,000 image fragments
exports about 33 ms more slowly, mostly in tokenization. The `html5gum`
tokenizer, used without optional features, adds no further dependencies and grows
the release executable by 231,952 bytes (1.3%).

Extending conversion beyond images left output unchanged, with no measurable time
difference, for 12,000 Markdown sections without HTML (1.5 MB) and 12,000 image
fragments (0.9 MB). Plain export of 12,000 title-and-tagline pairs (1.6 MB), which
now convert, measured 47.5 ms against 18.6 ms as source; 12,000 unsupported
`<table>` blocks containing `<b>` (0.8 MB) measured 15.5 ms against 11.4 ms
(medians of seven alternating runs, Apple M2 Pro, Rust 1.99).

## Backslash math

`\(...\)` denotes inline math and `\[...\]` denotes display math. Both share the
existing native rendering, TeX fallbacks, and selectable-source options. Multiline
math can occur within one Markdown paragraph, including a quote. Quote prefixes
are removed from the TeX passed to the math engine, while source exports retain
them. Table cells support these delimiters too.

Recognition excludes code spans, code fences, indented code, existing dollar math,
HTML, and link destinations. An escaped backslash does not open or close math:
`\\(literal\\)` stays ordinary text. Unmatched, empty, or oversized pairs retain
ordinary Markdown behavior. A pair cannot cross a paragraph or table-cell
boundary. Each pair is limited to 16 KiB, with at most 16,384 replacements and
1 MiB of retained TeX across the document.

The source adapter only runs on extension candidates. It projects recognized
syntax into parser input and maps resulting spans back to the original document.
The original Markdown remains authoritative. Documents using projections can
retain a second source-sized string; range discovery still scales with input.
These are bounded extensions, not a constant-memory Markdown parser.

## Clickable terminal links

`--links auto` emits OSC 8 links in directly recognized Ghostty/Kitty sessions.
`--links off` disables them; `--links always` enables them in another compatible
terminal. Pipes, explicit text/source output, dumb terminals, and recognized
multiplexers bypass hyperlinks. `NO_COLOR` disables colors without disabling
links; the two controls are independent.

Labels, including styled and wrapped pieces, link to their target. The visible
`(target)` is retained for selection and text export. Hyperlinks close before
padding, line boundaries, and graphics; equation pixels are not clickable.

Targets must be at most 2,048 bytes, use an absolute `http:`, `https:`, or `mailto:`
scheme, and contain no whitespace, backslashes, or control characters. Empty web
authorities are rejected. Other schemes, fragments, relative paths, and invalid
targets remain readable text. This is a bounded emission policy, not a browser
URL resolver or a guarantee about a destination's content. The viewer never opens
links automatically.

## References

- [GitHub alerts](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#alerts)
- [Front matter convention](https://jekyllrb.com/docs/step-by-step/03-front-matter/)
- [GitHub details sections](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/organizing-information-with-collapsed-sections)
- [OSC 8 specification](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)
