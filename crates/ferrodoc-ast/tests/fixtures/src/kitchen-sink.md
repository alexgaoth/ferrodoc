---
title: Kitchen Sink
published: true
authors:
  - Alice
  - Bob
options:
  draft: false
abstract: |
  First abstract paragraph.

  Second abstract paragraph.
---

# Heading one {#custom-id .cls key=val}

A paragraph with *emph*, **strong**, ~~strikeout~~, super^script^,
sub~script~, `code`{.rust}, "smart quotes", 'single quotes', a
[link](https://example.com "the title"), an inline image
![alt text](img.png "img title") in a sentence, a footnote^[The note
text.], math $e=mc^2$ and $$\int_0^1 x\,dx$$, citations [@doe2020] and
@smith2019, raw <kbd>html</kbd>, and a hard\
break.

[Spanned text]{#sid .scls k=v}, [small caps]{.smallcaps}, and
[underlined]{.underline}.

> A block quote.

    indented code block

``` {.rust #codeid}
fn fenced() {}
```

| line block one
| line block two

1. first decimal
2. second decimal

a. lower alpha
b. lower beta

i) lower roman
ii) lower roman two

(2) decimal two parens
(3) decimal three parens

#. default style one
#. default style two

@. example one
@. example two

- bullet
- bullet two
  - nested tight

term
:   definition one
:   definition two

second term
:   another definition

| Left | Center | Right | Default |
|:-----|:------:|------:|---------|
| a    | b      | c     | d       |

---

<!-- a raw HTML block -->

::: {#divid .fenced-div}
Div content.
:::

![A figure caption.](fig.png)

Ending paragraph.
