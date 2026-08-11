# ferrodoc

A *universal* document converter, written in **Rust**.

## Installation

Install with cargo:

```bash
cargo install ferrodoc
```

Or download a [release](https://example.com/releases "release page").

## Features

- Lossless pandoc-JSON interop
- Fast, in-process conversion
  - No subprocess per document
  - Embeddable in Python and Node
- Small memory footprint

1. Parse
2. Transform
3. Write

## Example

Convert markdown to HTML:

    ferrodoc -f markdown -t html README.md

> Note: this is an early release.
> Expect sharp edges.

---

See the `LICENSE` file. Logo: ![logo](logo.png "the logo")
