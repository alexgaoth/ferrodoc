//! Plain-text writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_text`] follows `pandoc -t plain --wrap=none`: inline markup is
//! dropped except strikeout's tildes, block structure is kept by
//! indentation, tables are column-aligned, and footnotes become numbered
//! references with their bodies at the end. Every rule below was probed
//! against pandoc 3.8.2.1 and is asserted on literal output, because no
//! differential gate reads this format — pandoc cannot parse it back.
//!
//! Not matched, and stated rather than hidden: pandoc renders `Math` as
//! Unicode (`$x^2$` becomes `x²`) where this writes the TeX, and it fills
//! to `--columns` where this never wraps.

use ferrodoc_ast::{Alignment, Block, Inline, Pandoc};
use std::fmt::Write as _;

/// The column a `HorizontalRule` fills and pandoc's default `--columns`.
const COLUMNS: usize = 72;

/// Render a document as plain text.
pub fn write_text(doc: &Pandoc) -> String {
    let mut writer = Writer::default();
    let mut paragraphs = Vec::new();
    writer.blocks(&doc.blocks, &mut paragraphs, "");
    for (index, body) in writer.notes.iter().enumerate() {
        paragraphs.push(format!("[{}] {body}", index + 1));
    }
    let mut out = paragraphs.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[derive(Default)]
struct Writer {
    /// Footnote bodies in reference order; written at the end as `[N] …`.
    notes: Vec<String>,
}

impl Writer {
    fn blocks(&mut self, blocks: &[Block], out: &mut Vec<String>, prefix: &str) {
        for block in blocks {
            self.block(block, out, prefix);
        }
    }

    fn block(&mut self, block: &Block, out: &mut Vec<String>, prefix: &str) {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                let text = self.inlines(inlines);
                out.push(indent(&text, prefix));
            }
            // Four spaces, which is what makes it read as code at all.
            Block::CodeBlock(_, text) => {
                out.push(indent(text.trim_end_matches('\n'), &format!("{prefix}    ")));
            }
            // Two more spaces per level, so nesting is visible.
            Block::BlockQuote(inner) => self.blocks(inner, out, &format!("{prefix}  ")),
            Block::Div(_, inner) => self.blocks(inner, out, prefix),
            Block::Figure(_, caption, inner) => {
                self.blocks(inner, out, prefix);
                self.blocks(&caption.blocks, out, prefix);
            }
            Block::BulletList(items) => self.list(items, out, prefix, |_| "- ".to_owned()),
            Block::OrderedList(attrs, items) => {
                let start = attrs.start;
                self.list(items, out, prefix, move |i| {
                    format!("{}.", start + i64::try_from(i).unwrap_or(0))
                });
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    let text = self.inlines(term);
                    out.push(indent(&text, prefix));
                    for definition in definitions {
                        self.blocks(definition, out, &format!("{prefix}    "));
                    }
                }
            }
            Block::LineBlock(lines) => {
                let text: Vec<String> = lines.iter().map(|l| self.inlines(l)).collect();
                out.push(indent(&text.join("\n"), prefix));
            }
            Block::Table(table) => {
                let rendered = self.table(table);
                if !rendered.is_empty() {
                    out.push(indent(&rendered, prefix));
                }
                self.blocks(&table.caption.blocks, out, prefix);
            }
            Block::HorizontalRule => out.push(indent(&"-".repeat(COLUMNS), prefix)),
            Block::RawBlock(..) => {}
        }
    }

    /// One paragraph per item when the list is loose, one for the whole
    /// list when it is tight — which is how pandoc keeps a tight list tight.
    fn list(
        &mut self,
        items: &[Vec<Block>],
        out: &mut Vec<String>,
        prefix: &str,
        marker: impl Fn(usize) -> String,
    ) {
        // An ordered list's marker column is as wide as its widest marker
        // plus a space, and never narrower than four: pandoc writes `1.  `
        // and `10. ` at the same width.
        let width = (0..items.len())
            .map(|i| marker(i).chars().count() + 1)
            .max()
            .unwrap_or(0)
            .max(4);
        let loose = items
            .iter()
            .any(|item| item.iter().any(|b| matches!(b, Block::Para(_))));
        let mut rendered = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mark = marker(index);
            // Bullets take exactly `- `; only ordered markers are padded.
            let (head, continuation) = if mark.ends_with(' ') {
                (mark.clone(), " ".repeat(mark.chars().count()))
            } else {
                (format!("{mark:<width$}"), " ".repeat(width))
            };
            let mut inner = Vec::new();
            self.blocks(item, &mut inner, "");
            let body = inner.join(if loose { "\n\n" } else { "\n" });
            let mut lines = body.split('\n');
            let mut text = format!("{}{}", head, lines.next().unwrap_or_default());
            for line in lines {
                text.push('\n');
                if !line.is_empty() {
                    text.push_str(&continuation);
                    text.push_str(line);
                }
            }
            rendered.push(indent(&text, prefix));
        }
        if loose {
            out.extend(rendered);
        } else if !rendered.is_empty() {
            out.push(rendered.join("\n"));
        }
    }

    /// A simple table: two-space margin, each column as wide as its widest
    /// cell plus two, a dashed rule under the head, and `AlignRight`
    /// columns padded on the left. Measured against pandoc cell by cell.
    fn table(&mut self, table: &ferrodoc_ast::Table) -> String {
        let head: Vec<Vec<String>> = self.rows(&table.head.rows);
        let body: Vec<Vec<String>> = table
            .bodies
            .iter()
            .flat_map(|b| [&b.head, &b.body])
            .chain(std::iter::once(&table.foot.rows))
            .flat_map(|rows| self.rows(rows))
            .collect();
        let columns = head.iter().chain(&body).map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                head.iter()
                    .chain(&body)
                    .filter_map(|row| row.get(c))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0)
                    + 2
            })
            .collect();
        let align = |c: usize| {
            table.colspecs.get(c).map_or(Alignment::AlignDefault, |s| s.alignment)
        };
        let line = |row: &Vec<String>| {
            let mut out = String::from("  ");
            for (c, width) in widths.iter().enumerate() {
                let cell = row.get(c).map_or("", String::as_str);
                if matches!(align(c), Alignment::AlignRight) {
                    let pad = width.saturating_sub(cell.chars().count());
                    out.push_str(&" ".repeat(pad));
                    out.push_str(cell);
                } else {
                    let _ = write!(out, "{cell:<width$}");
                }
                if c + 1 < widths.len() {
                    out.push(' ');
                }
            }
            out.trim_end().to_owned()
        };
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        let mut lines: Vec<String> = head.iter().map(line).collect();
        lines.push(format!("  {}", rule.join(" ")));
        lines.extend(body.iter().map(line));
        lines.join("\n")
    }

    fn rows(&mut self, rows: &[ferrodoc_ast::Row]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| {
                        let mut inner = Vec::new();
                        self.blocks(&cell.blocks, &mut inner, "");
                        inner.join(" ")
                    })
                    .collect()
            })
            .collect()
    }

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut out = String::new();
        self.collect(&mut out, inlines);
        out
    }

    fn collect(&mut self, out: &mut String, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
                Inline::Space | Inline::SoftBreak => out.push(' '),
                Inline::LineBreak => out.push('\n'),
                // The one markup pandoc keeps: without the tildes the text
                // says the opposite of what it means.
                Inline::Strikeout(inner) => {
                    out.push_str("~~");
                    self.collect(out, inner);
                    out.push_str("~~");
                }
                // A picture has no text, so its alt stands in brackets —
                // otherwise it reads as a sentence the document never had.
                Inline::Image(_, alt, _) => {
                    out.push('[');
                    self.collect(out, alt);
                    out.push(']');
                }
                Inline::Emph(i)
                | Inline::Strong(i)
                | Inline::Superscript(i)
                | Inline::Subscript(i)
                | Inline::SmallCaps(i)
                | Inline::Underline(i)
                | Inline::Quoted(_, i)
                | Inline::Cite(_, i)
                | Inline::Span(_, i)
                | Inline::Link(_, i, _) => self.collect(out, i),
                Inline::Note(blocks) => {
                    // Reserve the number before rendering, so a note inside
                    // a note cannot take this one's.
                    let index = self.notes.len();
                    self.notes.push(String::new());
                    let mut inner = Vec::new();
                    self.blocks(blocks, &mut inner, "");
                    self.notes[index] = inner.join(" ");
                    let _ = write!(out, "[{}]", index + 1);
                }
                Inline::RawInline(..) => {}
            }
        }
    }
}

/// Put `prefix` on every non-empty line of `text`.
fn indent(text: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| if line.is_empty() { String::new() } else { format!("{prefix}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, Caption, Cell, ColSpec, ColWidth, ListAttributes, ListNumberDelim,
        ListNumberStyle, Row, Table, TableBody, TableFoot, TableHead, Target};

    fn ordered() -> ListAttributes {
        ListAttributes {
            start: 1,
            style: ListNumberStyle::Decimal,
            delim: ListNumberDelim::Period,
        }
    }

    fn plain(inlines: Vec<Inline>) -> Block {
        Block::Plain(inlines)
    }
    fn str_(s: &str) -> Inline {
        Inline::Str(s.to_owned())
    }

    /// Every assertion here is `pandoc -t plain --wrap=none` output, run
    /// and pasted. Nothing else can check this writer: pandoc cannot read
    /// plain text back, so there is no differential gate at all.
    #[test]
    fn formatting_is_dropped_and_a_tight_list_stays_tight() {
        let doc = Pandoc::new(vec![
            Block::Header(1, Attr::default(), vec![str_("Title")]),
            Block::Para(vec![
                Inline::Emph(vec![str_("em")]),
                Inline::Space,
                Inline::Link(
                    Box::default(),
                    vec![str_("link")],
                    Box::new(Target { url: "u".into(), title: String::new() }),
                ),
            ]),
            Block::BulletList(vec![vec![plain(vec![str_("a")])], vec![plain(vec![str_("b")])]]),
        ]);
        assert_eq!(write_text(&doc), "Title\n\nem link\n\n- a\n- b\n");

        // A `Para` inside any item makes the whole list loose.
        let loose = Pandoc::new(vec![Block::BulletList(vec![
            vec![Block::Para(vec![str_("a")])],
            vec![Block::Para(vec![str_("b")])],
        ])]);
        assert_eq!(write_text(&loose), "- a\n\n- b\n");
    }

    #[test]
    fn a_quote_is_indented_two_and_code_four() {
        let doc = Pandoc::new(vec![Block::BlockQuote(vec![
            Block::Para(vec![str_("outer")]),
            Block::BlockQuote(vec![Block::Para(vec![str_("inner")])]),
        ])]);
        assert_eq!(write_text(&doc), "  outer\n\n    inner\n");

        let code = Pandoc::new(vec![Block::CodeBlock(Attr::default(), "one\ntwo\n".into())]);
        assert_eq!(write_text(&code), "    one\n    two\n");
    }

    #[test]
    fn an_ordered_marker_is_padded_to_a_common_width() {
        // pandoc writes `1.  one` and `10. ten` — the column is as wide as
        // the widest marker plus a space, and never under four.
        let items: Vec<Vec<Block>> =
            (1..=10).map(|n| vec![plain(vec![str_(&format!("i{n}"))])]).collect();
        let doc = Pandoc::new(vec![Block::OrderedList(ordered(), items)]);
        let text = write_text(&doc);
        assert!(text.starts_with("1.  i1\n"), "{text}");
        assert!(text.contains("\n10. i10\n"), "{text}");

        // Continuation lines line up under the content, not the marker.
        let wrapped = Pandoc::new(vec![Block::OrderedList(
            ordered(),
            vec![vec![plain(vec![str_("a")]), plain(vec![str_("b")])]],
        )]);
        assert_eq!(write_text(&wrapped), "1.  a\n    b\n");
    }

    #[test]
    fn a_table_is_column_aligned_with_a_rule_under_the_head() {
        let cell = |s: &str| Cell {
            attr: Attr::default(),
            alignment: Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![plain(vec![str_(s)])],
        };
        let row = |cells: Vec<Cell>| Row { attr: Attr::default(), cells };
        let spec = |a| ColSpec { alignment: a, width: ColWidth::ColWidthDefault };
        let table = Table {
            attr: Attr::default(),
            caption: Caption::default(),
            colspecs: vec![spec(Alignment::AlignDefault), spec(Alignment::AlignRight)],
            head: TableHead {
                attr: Attr::default(),
                rows: vec![row(vec![cell("Phase"), cell("Starts")])],
            },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: vec![
                    row(vec![cell("Inventory"), cell("2026-01-06")]),
                    row(vec![cell("Pilot"), cell("2026-02-17")]),
                ],
            }],
            foot: TableFoot { attr: Attr::default(), rows: Vec::new() },
        };
        // Widths are the widest cell plus two; the right-aligned column
        // pads on the left; trailing space is trimmed.
        assert_eq!(
            write_text(&Pandoc::new(vec![Block::Table(Box::new(table))])),
            concat!(
                "  Phase             Starts\n",
                "  ----------- ------------\n",
                "  Inventory     2026-01-06\n",
                "  Pilot         2026-02-17\n",
            )
        );
    }

    #[test]
    fn a_footnote_becomes_a_number_and_a_body_at_the_end() {
        let note = |s: &str| Inline::Note(vec![Block::Para(vec![str_(s)])]);
        let doc = Pandoc::new(vec![Block::Para(vec![
            str_("a"),
            note("one"),
            Inline::Space,
            str_("b"),
            note("two"),
        ])]);
        assert_eq!(write_text(&doc), "a[1] b[2]\n\n[1] one\n\n[2] two\n");
    }

    #[test]
    fn strikeout_keeps_its_tildes_and_a_picture_keeps_its_brackets() {
        // Without the tildes the sentence says the opposite of what it
        // means; without the brackets an alt text reads as prose.
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Strikeout(vec![str_("gone")]),
            Inline::Space,
            Inline::Image(
                Box::default(),
                vec![str_("the logo")],
                Box::new(Target { url: "l.png".into(), title: String::new() }),
            ),
        ])]);
        assert_eq!(write_text(&doc), "~~gone~~ [the logo]\n");
    }

    #[test]
    fn a_horizontal_rule_is_seventy_two_dashes() {
        let doc = Pandoc::new(vec![Block::HorizontalRule]);
        assert_eq!(write_text(&doc), format!("{}\n", "-".repeat(72)));
    }
}
