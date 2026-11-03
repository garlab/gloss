"""Generate deterministic benchmark inputs in an external scratch directory."""

from pathlib import Path
import argparse
import hashlib
import json
import shutil
import struct
import zlib

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

# Small reproducible assets and documents; generated data stays in WORK_DIR.
assets = CORPORA / "assets"
assets.mkdir(exist_ok=True)
for fixture in (PROJECT / "tests/fixtures/assets").iterdir():
    shutil.copyfile(fixture, assets / fixture.name)

def png(name, width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    row = b"\0" + bytes((65, 145, 195, 255)) * width
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(row * height)) + chunk(b"IEND", b"")
    (assets / name).write_bytes(data)

png("large.png", 2000, 2000)
png("budget.png", 400, 200)
(assets / "invalid.png").write_bytes(b"not a PNG")
(assets / "filtered.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg"><filter id="f"/></svg>\n')
(assets / "oversized.svg").write_text(" " * (256 * 1024 + 1))
for i in range(32):
    (assets / f"vector-{i}.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">'
        f'<rect width="120" height="60" fill="rgb({i * 8},145,195)"/></svg>\n')

image_cases = {
    "images-png.md": ("![PNG](assets/color-grid.png)\n", 1),
    "images-jpeg.md": ("![JPEG](assets/gradient.jpg)\n", 1),
    "images-svg.md": ("![Geometry](assets/vector-0.svg)\n", 1),
    "images-svg-text.md": ("![Labels](assets/diagram.svg)\n", 1),
    "images-repeated-100.md": ("![Cached](assets/color-grid.png)\n\n" * 100, 100),
    "images-unique-32.md": ("".join(f"![Vector {i}](assets/vector-{i}.svg)\n\n" for i in range(32)), 32),
    "images-large.md": ("![Four million pixels](assets/large.png)\n", 1),
    "images-invalid.md": (("![Invalid](assets/invalid.png)\n\n![Filter](assets/filtered.svg)\n\n"
        "![Oversized](assets/oversized.svg)\n\n![Remote](https://example.com/image.png)\n\n"
        "![Missing](assets/missing.png)\n\n") * 10 + "![Valid](assets/color-grid.png)\n", 1),
    # 400*200*4 cell-padded bytes per image, capped at 64 MiB per invocation.
    "images-budget-1000.md": ("![Bounded](assets/budget.png)\n\n" * 1000, 209),
}
asset_hashes = {"assets/" + path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted(assets.iterdir()) if path.is_file()}
for name, (content, images) in image_cases.items():
    (CORPORA / name).write_text(content, encoding="utf-8")
    metadata[name] = {"images": images, "assets": asset_hashes,
                      "description": "Local image decoding, caching, fitting, or budget control."}

flow = "flowchart LR\n A[Read] --> B[Render] --> C[Show]\n"
sequence = "sequenceDiagram\n Reader->>Viewer: Open file\n Viewer-->>Reader: Show document\n"
def fence(source):
    return "```mermaid\n" + source + "\n```\n\n"
mermaid_cases = {
    "mermaid-basic.md": ((PROJECT / "tests/fixtures/mermaid.md").read_text(), 13),
    "mermaid-flow.md": (fence(flow), 1),
    "mermaid-sequence.md": (fence(sequence), 1),
    "mermaid-repeated-100.md": (fence(flow) * 100, 100),
    "mermaid-unique-40.md": ("".join(fence(f"flowchart LR; A[Diagram {i}]-->B") for i in range(40)), 32),
    "mermaid-complex.md": (fence("flowchart TD\n" + "".join(
        f" N{i}[Stage {i}]-->N{i+1}\n N{i}-->N{i+2}\n" for i in range(30))), 1),
    "mermaid-fallbacks.md": (fence("flowchart LR\n" + "A" * (64 * 1024))
        + fence("pie\n\"A\":1") + fence("flowchart LR; A[中文]-->B")
        + fence("flowchart LR; A[Broken") + fence("%%{init: {}}%%\nflowchart LR; A-->B")
        + fence("erDiagram\n A {\n string name\n }")
        + fence("flowchart LR\n" + "".join(f"N{i}[Node {i}]\n" for i in range(513)))
        + fence(flow), 3),
}
for name, (content, diagrams) in mermaid_cases.items():
    (CORPORA / name).write_text(content, encoding="utf-8")
    metadata[name] = {"diagrams": diagrams, "description": "Mermaid native subset, cache, worker, or fallback control."}

extensions = (PROJECT / "tests/fixtures/extensions.md").read_text()
extension_cases = {
    "extensions-basic.md": (extensions, 2),
    "extensions-repeated-1000.md": ("Before \\(x^2+1\\) after.\n\n" * 1000, 1000),
    "extensions-details-1m.md": (("<details>\n<summary>More information</summary>\n\n"
        "> [!NOTE]\n> Read the [guide](https://example.com/guide).\n\n"
        "**Expanded** body and `literal code`.\n\n</details>\n\n") * 7000, 0),
    "extensions-over-limit.md": ("---\ntitle: " + "x" * (17 * 1024) + "\n---\n\n"
        + "<details>\n<summary>Large</summary>\n\n" + "text " * (60 * 1024) + "\n</details>\n\n"
        + "\\(" + "x" * (17 * 1024) + "\\)\n\n"
        + "```text\n\\(code\\)\n<details>literal</details>\n```\n", 0),
}
for name, (content, equations) in extension_cases.items():
    (CORPORA / name).write_text(content, encoding="utf-8")
    metadata[name] = {"equations": equations, "description": "Markdown extensions, projections, links, or source fallback."}

for name, info in metadata.items():
    data = (CORPORA / name).read_bytes()
    info.update(bytes=len(data), lines=data.count(b"\n"), sha256=hashlib.sha256(data).hexdigest())
(ROOT / "corpora.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
print(json.dumps(metadata, indent=2))
