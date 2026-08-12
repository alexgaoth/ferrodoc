 Convert ordinary editorial documents—Markdown, DOCX, and HTML—quickly, locally, and predictably.

  That is a valuable boundary. It excludes the Pandoc long tail: PDF, LaTeX, presentations, citations, templates, and every office-workflow feature.

  A sensible roadmap:

  1. Make the current promise trustworthy.
      - CI on Linux/macOS/Windows, pinned against the supported Pandoc version.
      - Publish a compatibility matrix, including known losses.
      - Add regression fixtures for every discovered DOCX mismatch.
      - Fuzz Markdown, XML, ZIP, and pathological nesting.
      - Publish benchmark results for 10 KB, 1 MB, and 10 MB documents, with latency and peak memory—not speed ratios alone.
      - Release a stable 0.1 API and binary.

  2. Finish the “ordinary DOCX” path before adding formats.

     Target DOCX documents containing:
      - headings, paragraphs, quotes, code, lists
      - links, notes/footnotes, tables, captions
      - basic character formatting and named styles
      - embedded images—the largest current practical gap
      - basic metadata and document properties

     Explicitly defer or declare lossy:
      - tracked changes/comments/reviewer workflows
      - forms/content controls
      - arbitrary page geometry, headers/footers, complex section breaks
      - SmartArt, charts, macros, embedded spreadsheets
      - pixel-perfect layout preservation

     The goal is semantic conversion, not “open any Word file and preserve every visual detail.”

  3. Add a Markdown writer.

     This is probably the highest-leverage new capability:

     DOCX → AST → Markdown
     HTML → AST → Markdown

     It makes the project useful for CMS migrations, Git-based documentation, and AI/RAG cleanup workflows. Support a documented dialect—CommonMark first, then an optional GitHub-
     Flavored Markdown mode for tables, task lists, and strikethrough.

  4. Add an HTML reader.

     This completes a practical triangle:

     Markdown ↔ AST ↔ HTML
                     ↕
                    DOCX

     Then users can convert web/CMS HTML into DOCX or Markdown. Keep it structural—headings, lists, tables, links, images, formatting—not arbitrary CSS/layout reproduction.

  5. Package only after the core surface is stable.
      - Release a static CLI binary first.
      - Release a Rust crate as the native API.
      - Make a browser WASM package if private client-side conversion is a target.
      - Add Python or Node bindings based on where actual users are.
