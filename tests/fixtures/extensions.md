---
title: Markdown extensions
tags: [terminal, equations]
---

# Markdown extensions

> [!NOTE]
> Alerts keep their labels with or without color.

> [!TIP]
> Use `--show-math-source` for selectable equation source.

> [!IMPORTANT]
> Source export preserves the original document.

> [!WARNING]
> Unsupported features retain readable text.

> [!CAUTION]
> Document content never runs commands.

## Expanded details

<details>
<summary>Derivation and **references**</summary>

The inline result is \(x^2+y^2=z^2\).

\[
\frac{a+b}{c} = \sqrt{1+x^2}
\]

- Read the [Markdown guide](https://www.markdownguide.org/).
- `\(this stays code\)`.

```rust
let message = "\\(not math\\)";
```

<details open>
<summary>A nested note</summary>

Details are expanded even without the `open` attribute.

</details>

</details>
