"""Generate deterministic benchmark inputs in an external scratch directory."""

from pathlib import Path
import argparse
import hashlib
import json

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--work-dir", type=Path, required=True)
args = parser.parse_args()
PROJECT = Path(__file__).resolve().parent.parent
ROOT = args.work_dir.expanduser().resolve()
if ROOT == PROJECT or PROJECT in ROOT.parents:
    parser.error("--work-dir must be outside the repository")
CORPORA = ROOT / "corpora"
CORPORA.mkdir(parents=True, exist_ok=True)
commonmark = CORPORA / "commonmark.md"
if not commonmark.is_file():
    parser.error("place CommonMark 0.31.2 spec.txt in WORK_DIR/corpora/commonmark.md first")
if hashlib.sha256(commonmark.read_bytes()).hexdigest() != "257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80":
    parser.error("CommonMark input does not match the pinned baseline")
(ROOT / "glow.yml").write_text("{}\n", encoding="utf-8")

def block(index):
    return f"""# Reference section {index}

This section mixes ordinary prose with **strong emphasis**, *italic text*, `inline_code`, ~~revised wording~~, and a [reference](https://example.com/reference/{index}). The paragraph includes Unicode such as café, é, 中文, and 👩‍💻 so wrapping must measure graphemes instead of counting bytes. A readable layout keeps these words together wherever the available width permits it.

## Lists, quotes, and tasks

> A quotation with multiple sentences and enough text to wrap at an eighty-column reading width. It includes **emphasis** and a [nested reference](https://example.com/guide).
>
> A second paragraph in the same quotation.

- An ordinary list item with a detailed explanation.
  - A nested item with `a_command --flag value` and more text that should wrap under its own content.
    - A third level with additional descriptive words.
- [x] A completed task.
- [ ] An incomplete task.

1. Read the source document.
2. Review the rendered content and its source mapping.

### Tables and code

| Component | Value | Description |
| :-- | --: | :-- |
| Parser | {index} | Retains original Markdown and source ranges. |
| Layout | {index + 1} | Wraps text, styled runs, and indivisible images. |
| Output | {index + 2} | Writes text or terminal graphics. |

```rust
fn evaluate_{index}(values: &[f64]) -> f64 {{
    values.iter()
        .enumerate()
        .map(|(i, value)| value * (i + 1) as f64)
        .sum()
}}
```

```json
{{"section": {index}, "enabled": true, "labels": ["alpha", "beta", "gamma"]}}
```

A note with an entity &amp; an escaped asterisk \\* and a footnote.[^note-{index}]

[^note-{index}]: A unique footnote definition with **styled content**.

---

"""

def text_corpus(name, target, make_block=block):
    pieces, size, index = [], 0, 0
    while size < target:
        piece = make_block(index)
        pieces.append(piece)
        size += len(piece.encode())
        index += 1
    (CORPORA / name).write_text("".join(pieces), encoding="utf-8")
    return {"sections": index}

def math_corpus(name, count, repeated=False):
    pieces = ["# Equation reference\n\n"]
    for index in range(count):
        value = 7 if repeated else index + 1
        if index % 4 != 3:
            templates = [
                rf"\frac{{x_{{{value}}}^2 + 2x_{{{value}}} + 1}}{{1+x_{{{value}}}^2}}",
                rf"\sqrt{{a_{{{value}}}^2+b_{{{value}}}^2}}",
                rf"\displaystyle\sum_{{k=1}}^{{n}} \frac{{k+{value}}}{{1+k^2}}",
            ]
            tex = templates[index % 4]
            pieces.append(f"Result {index + 1}: ${tex}$ is shown beside ordinary explanatory text.\n\n")
        else:
            templates = [
                rf"\begin{{aligned}} f_{{{value}}}(x)&=x^2+{value}x+1\\ f'_{{{value}}}(x)&=2x+{value} \end{{aligned}}",
                rf"\int_0^\infty e^{{-a_{{{value}}}t}}\,dt = \frac{{1}}{{a_{{{value}}}}}",
                rf"\begin{{pmatrix}} a & b \\ c & d \end{{pmatrix}}\begin{{pmatrix}} x \\ y \end{{pmatrix}} = \begin{{pmatrix}} ax+by \\ cx+dy \end{{pmatrix}}\tag{{{value}}}",
                rf"g_{{{value}}}(x)=\begin{{cases}}x^2 & x \ge 0\\-x & x<0\end{{cases}}",
            ]
            tex = templates[(index // 4) % len(templates)]
            pieces.append(f"$$\n{tex}\n$$\n\n")
    (CORPORA / name).write_text("".join(pieces), encoding="utf-8")
    return {"equations": count, "inline": count - count // 4, "display": count // 4}

metadata = {
    "commonmark.md": {"source": "https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt"},
    "mixed-256k.md": text_corpus("mixed-256k.md", 256 * 1024),
    "mixed-1m.md": text_corpus("mixed-1m.md", 1024 * 1024),
    "mixed-5m.md": text_corpus("mixed-5m.md", 5 * 1024 * 1024),
    "mixed-10m.md": text_corpus("mixed-10m.md", 10 * 1024 * 1024),
    "math-100.md": math_corpus("math-100.md", 100),
    "math-1000.md": math_corpus("math-1000.md", 1000),
    "math-5000.md": math_corpus("math-5000.md", 5000),
    "math-repeated-1000.md": math_corpus("math-repeated-1000.md", 1000, repeated=True),
}
(CORPORA / "long-lines-1m.md").write_text("# Long lines\n\n" + "word " * 100_000 + "\n\n" + "x" * 524_288 + "\n", encoding="utf-8")
metadata["long-lines-1m.md"] = {"description": "A 500 KB paragraph followed by a 512 KiB unbroken word."}

# Additional feature controls; the original ten inputs remain byte-identical.
def prose_block(index):
    return f"""## Reading section {index}

Ordinary prose with **emphasis**, [links](https://example.com), and Unicode café,
中文, and 👩‍💻. These paragraphs contain no fenced code or math.

- A list item with enough text to exercise the normal wrapping path.
- Another item to preserve the distinction between code and ordinary formatting.

"""

for name, size in [("prose-1m.md", 1024 * 1024), ("prose-10m.md", 10 * 1024 * 1024)]:
    metadata[name] = text_corpus(name, size, prose_block)
code_cases = {
    "code-languages.md": (PROJECT / "tests/fixtures/code.md").read_text(encoding="utf-8") * 100,
    "code-multiline.md": "```rust\n/*\n" + "multiline comment 中文\n" * 4000 + "*/\nfn main() {}\n```\n",
    "code-over-limit.md": "```rust\n" + "let value = 42;\n" * 10000 + "```\n",
    "code-captures.md": "```json\n[" + "1," * 20000 + "0]\n```\n",
}
for name, content in code_cases.items():
    (CORPORA / name).write_text(content, encoding="utf-8")
    metadata[name] = {"description": "Syntax highlighting feature/control input."}

def table_corpus(name, target):
    size, rows = 0, 0
    with (CORPORA / name).open("w", encoding="utf-8", newline="\n") as output:
        output.write("| Component | Value | Description |\n| :--- | ---: | :---: |\n")
        while size < target:
            row = f"| item-{rows} | {rows} | Text with **emphasis**, `code`, café, 中文, and 👩‍💻 to wrap. |\n"
            output.write(row)
            size += len(row.encode())
            rows += 1
    return {"rows": rows, "description": "A single large table; width sampling must remain bounded."}

for name, size in [("tables-1m.md", 1024 * 1024), ("tables-10m.md", 10 * 1024 * 1024)]:
    metadata[name] = table_corpus(name, size)
table_cases = {
    "tables-wide.md": "| " + " | ".join(f"Column {i}" for i in range(128)) + " |\n"
        + "| " + " | ".join(["---"] * 128) + " |\n"
        + ("| " + " | ".join(f"value-{i}" for i in range(128)) + " |\n") * 16,
    "tables-over-limit.md": "| A | B | C | D | E |\n| --- | --- | --- | --- | --- |\n"
        + "| " + " | ".join(["x" * (60 * 1024)] * 5) + " |\n"
        + "| " + "y" * (70 * 1024) + " | a | b | c | d |\n| AFTER | 1 | 2 | 3 | 4 |\n",
    "tables-math-100.md": "| Fraction | Root |\n| :---: | ---: |\n" + "".join(
        rf"| $\frac{{a_{{{i}}}}}{{b}}$ | $\sqrt{{x_{{{i}}}}}$ |" + "\n" for i in range(50)
    ),
}
for name, content in table_cases.items():
    (CORPORA / name).write_text(content, encoding="utf-8")
    metadata[name] = {"description": "Table layout feature/control input."}
metadata["tables-math-100.md"]["equations"] = 100

for name, info in metadata.items():
    data = (CORPORA / name).read_bytes()
    info.update(bytes=len(data), lines=data.count(b"\n"), sha256=hashlib.sha256(data).hexdigest())
(ROOT / "corpora.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
print(json.dumps(metadata, indent=2))
