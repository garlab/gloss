# Local images

A vector diagram with selectable alt text and its source reference:

![A line chart with an explanatory label](assets/diagram.svg)

A PNG with transparency:

![A blue color grid](assets/color-grid.png)

A JPEG:

![A warm color gradient](assets/gradient.jpg)

> A quoted image keeps its vertical rail.
>
> ![Diagram in a quote](assets/diagram.svg)

- ![Image in a list](assets/color-grid.png)

HTML image markup renders like the equivalent Markdown:

<p align="center">
  <img src="assets/color-grid.png" alt="A centered grid" width="120">
</p>

Remote references stay as text, including in HTML:

<img src="https://example.com/banner.gif" alt="Remote banner">

![Remote example](https://example.com/image.png)

An inline reference ![Inline diagram](assets/diagram.svg) becomes a block image
when graphics are enabled. Plain export keeps the original text flow.
