# Markdown and equations

Ordinary **bold**, *italic*, and `code` remain terminal text.
Inline math can render as equations or selectable TeX: $E = mc^2$ and $\alpha + \beta$.

## Display equations

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

```math
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{pmatrix} x \\ y \end{pmatrix}
= \begin{pmatrix} ax+by \\ cx+dy \end{pmatrix}
```

$$
\begin{equation} e^{i\pi} + 1 = 0 \end{equation}
$$

## Text and copying

- Use `--show-math-source` in Ghostty or Kitty to see and copy the TeX.
- Pipe output to `pbcopy` for clean text, or use `--output source` for exact Markdown.
- [KaTeX supported functions](https://katex.org/docs/supported.html)

```rust
let literal = "$this is code, not math$";
```
