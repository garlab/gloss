# Field notes

A comfortable reading column makes long documents easier to follow. Use **strong emphasis** for the important parts, *italics* for a quieter aside, and `inline code` for names and commands. A long paragraph should stay readable on a wide screen and reflow naturally when the terminal is narrow.

## Ideas worth keeping

> Good typography leaves room to breathe. A quotation can wrap across several lines while keeping its place in the document.
>
> A second paragraph belongs to the same quotation.

- A short item with **emphasis**.
- A longer item that wraps beneath its text instead of jumping back under the bullet when the available reading width gets smaller.
  - A nested item with `a command` and enough text to wrap at a narrower width.

1. Read the document.
2. Check the equations and copy their source when needed.

### Code and links

```rust
fn energy(mass: f64, speed: f64) -> f64 {
    mass * speed.powi(2)
}
```

Read the [project documentation](https://example.com/docs) for details. ~~Outdated notes~~ can remain visible while the newer explanation takes their place.

---

## Equations in context

Mass and energy obey $E = mc^2$. Fractions such as $\frac{a+b}{c}$ and roots such as $\sqrt{1+x^2}$ share the same reading column as the surrounding prose.

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$
