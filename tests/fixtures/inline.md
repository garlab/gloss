# Inline equations

Mass and energy obey $E = mc^2$. The surrounding sentence remains terminal text.

Fractions such as $\frac{1}{1+x}$, roots such as $\sqrt{1+x^2}$, and powers such as
$x^{n+1}$ should share a baseline with these words.

Taller expressions need more room: $\displaystyle\sum_{k=1}^{n} k = \frac{n(n+1)}{2}$
and $\displaystyle\frac{1}{1+\frac{1}{x}}$ must leave the lines above and below readable.

Several adjacent expressions also work: $a$+$b$=$c$. Punctuation stays with the
formula where the available width permits it.

Unicode text stays intact while wrapping: café, é, 中文, and 👩‍💻 next to $\alpha+\beta$.

## Narrow windows

This sentence is deliberately long enough to wrap in a narrow terminal, including
$\frac{x+y+z}{1+x^2+y^2+z^2}$ between ordinary words. A formula is kept together
and is reduced in size if it cannot fit on a line.

## Copying

Use `--show-math-source` to print selectable TeX under the corresponding line.
Use `--math tex` to keep equations as text, or `--output source` to export the
original document. Code such as `$not_math$` remains literal.
