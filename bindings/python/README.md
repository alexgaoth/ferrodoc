# ferrodoc

Convert documents between markdown (CommonMark and GFM), HTML and DOCX —
in your process, without shelling out to pandoc.

```python
import ferrodoc

with open("report.docx", "rb") as f:
    markdown = ferrodoc.convert(f.read(), "docx", "gfm")
```

Built on the Rust [ferrodoc](https://github.com/alexgaoth/ferrodoc) crates,
whose output is checked against pandoc 3.8.2.1 document by document.
