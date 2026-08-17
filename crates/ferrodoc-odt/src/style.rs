//! The style tables an ODF document's formatting is expressed through.
//!
//! Almost nothing in an ODF body carries its own formatting: a paragraph or
//! a span names a style, and the style says what it means. Styles live in
//! two places at once — `styles.xml` holds the named ones a user picks from
//! ("Quotations", "Heading 1") and `content.xml` holds automatic ones the
//! editor generates per run (`T1`, `P4`) — and both are looked up in the
//! same namespace, so they are collected into one table here.
//!
//! Names are the `style:name` attribute, not `style:display-name`: the body
//! refers to a style by the former, and the former is the ODF-escaped
//! spelling (`Text_20_body`), which is what pandoc compares against too.

use ferrodoc_docx::xml::Node;
use std::collections::{HashMap, HashSet};

/// Every style table a document declares, merged across its parts.
#[derive(Default)]
pub struct Styles {
    /// `style:name` to what that style says.
    by_name: HashMap<String, Style>,
    /// A list style's levels, deepest last, by `style:name`.
    lists: HashMap<String, Vec<Level>>,
    /// The font faces declared `style:font-pitch="fixed"`.
    fixed_fonts: HashSet<String>,
}

/// One `style:style` declaration.
#[derive(Default)]
struct Style {
    parent: Option<String>,
    text: TextProps,
    /// `fo:margin-left`, in millimetres.
    margin_left: Option<f64>,
}

/// The subset of `style:text-properties` that changes the AST.
///
/// Every field is a three-way answer, because a style that says nothing
/// about a property leaves whatever an enclosing span said standing, and a
/// style that says `normal` turns it off again.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct TextProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    position: Option<Position>,
    fixed_pitch: Option<bool>,
}

/// Whether a run is raised, lowered, or on the baseline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// Raised: `super`, or a positive percentage.
    Super,
    /// Lowered: `sub`, or a negative percentage.
    Sub,
    /// On the baseline: `0%`.
    Normal,
}

/// One level of a `text:list-style`.
#[derive(Clone)]
pub struct Level {
    /// The numbering format, or `None` for a bullet.
    pub number: Option<NumberFormat>,
    /// `text:start-value`.
    pub start: i64,
}

/// A numbered level's format and the delimiter around it.
#[derive(Clone)]
pub struct NumberFormat {
    /// `style:num-format`: `1`, `a`, `A`, `i` or `I`.
    pub format: String,
    /// `style:num-prefix`.
    pub prefix: String,
    /// `style:num-suffix`.
    pub suffix: String,
}

/// The character style pandoc's own ODT writer uses for inline code, and
/// the only thing its reader recognizes as code.
///
/// Measured: a style *named* this reads back as `Code` whatever properties
/// it carries, and a style carrying `Source_Text`'s exact properties under
/// any other name does not. A monospaced font is not what makes code here.
pub const CODE_STYLE: &str = "Source_Text";

/// How far a paragraph must be indented to become a block quote, in
/// millimetres. Measured by bisecting `fo:margin-left` against the binary:
/// 5.4999 mm is a paragraph and 5.5 mm is a quote.
const QUOTE_INDENT_MM: f64 = 5.5;

/// A length in any of ODF's units, in millimetres.
fn length(value: &str) -> Option<f64> {
    let split = value.find(|c: char| c.is_ascii_alphabetic() || c == '%')?;
    let (number, unit) = value.split_at(split);
    let per_unit = match unit {
        "mm" => 1.0,
        "cm" => 10.0,
        "in" => 25.4,
        "pt" => 25.4 / 72.0,
        "pc" => 25.4 / 6.0,
        "px" => 25.4 / 96.0,
        // A percentage or a font-relative length has no absolute size, and
        // guessing one would classify a paragraph on a number that means
        // something else.
        _ => return None,
    };
    Some(number.trim().parse::<f64>().ok()? * per_unit)
}

impl Styles {
    /// Fold one part's `office:styles`, `office:automatic-styles` and
    /// `office:font-face-decls` into the table.
    pub fn absorb(&mut self, root: &Node) {
        for container in root.elems() {
            match container.name.as_str() {
                "font-face-decls" => {
                    for face in container.children_named("font-face") {
                        if face.attr("style:font-pitch") == Some("fixed")
                            && let Some(name) = face.attr("style:name")
                        {
                            self.fixed_fonts.insert(name.to_owned());
                        }
                    }
                }
                "styles" | "automatic-styles" => {
                    for style in container.elems() {
                        let Some(name) = style.attr("style:name") else { continue };
                        match style.name.as_str() {
                            "style" => {
                                self.by_name.insert(name.to_owned(), self.read_style(style));
                            }
                            "list-style" => {
                                self.lists.insert(name.to_owned(), read_levels(style));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn read_style(&self, style: &Node) -> Style {
        let props = style
            .child("text-properties")
            .map(|p| self.read_text_props(p))
            .unwrap_or_default();
        Style {
            parent: style.attr("style:parent-style-name").map(str::to_owned),
            text: props,
            margin_left: style
                .child("paragraph-properties")
                .and_then(|p| p.attr("fo:margin-left"))
                .and_then(length),
        }
    }

    fn read_text_props(&self, props: &Node) -> TextProps {
        TextProps {
            // Any weight that is a number at all is bold, including `100`;
            // only `normal` and unparseable words are not. Measured one
            // value at a time, because `bolder` is *not* bold here.
            bold: props.attr("fo:font-weight").map(|w| {
                w == "bold" || w.parse::<f64>().is_ok()
            }),
            italic: props
                .attr("fo:font-style")
                .map(|s| matches!(s, "italic" | "oblique")),
            underline: props
                .attr("style:text-underline-style")
                .map(|s| s != "none"),
            strike: props
                .attr("style:text-line-through-style")
                .map(|s| s != "none"),
            position: props.attr("style:text-position").map(position),
            fixed_pitch: props
                .attr("style:font-name")
                .map(|font| self.fixed_fonts.contains(font)),
        }
    }

    /// The text properties a style declares *itself*.
    ///
    /// Deliberately not inherited through `style:parent-style-name`: a
    /// style whose parent is bold and which adds italic reads back as
    /// italic alone. Measured, and the reason a child of `Source_Text` is
    /// not code.
    pub fn text_props(&self, name: &str) -> TextProps {
        self.by_name.get(name).map(|s| s.text).unwrap_or_default()
    }

    /// Whether a paragraph in this style is indented far enough to be a
    /// block quote.
    ///
    /// The *indent*, not the style's name, is what makes a block quote.
    /// `Quotations` qualifies because its definition carries
    /// `fo:margin-left="0.3937in"`; a style named Quotations without one is
    /// an ordinary paragraph, and a `Preformatted Text` paragraph with one
    /// is a quote — which is how a code block inside a quote comes back.
    /// The threshold is exactly [`QUOTE_INDENT_MM`], measured by bisection:
    /// `Table Contents` (0.76 mm) and `Footnote` (5.0 mm) both sit under it
    /// and are ordinary paragraphs, which is the whole reason it exists.
    ///
    /// The margin is the **largest** declared anywhere in the parent chain,
    /// not the nearest: a style that re-declares `0in` over an indented
    /// parent is still a quote, and two 0.15in steps do not add up to one.
    pub fn is_indented(&self, name: &str) -> bool {
        let mut current = name;
        let mut margin = 0.0f64;
        // Bounded rather than trusting the file: a style naming itself as
        // its own parent is a two-line loop that never returns.
        for _ in 0..MAX_STYLE_DEPTH {
            let Some(style) = self.by_name.get(current) else { break };
            margin = margin.max(style.margin_left.unwrap_or(0.0));
            match style.parent.as_deref() {
                Some(parent) => current = parent,
                None => break,
            }
        }
        margin >= QUOTE_INDENT_MM
    }

    /// The `depth`-th level of a list style, counting from 1.
    pub fn list_level(&self, name: &str, depth: usize) -> Option<Level> {
        let levels = self.lists.get(name)?;
        // A list nested deeper than its style defines levels takes the
        // deepest one declared, which is what an editor's ten-level style
        // means by its last entry.
        levels.get(depth - 1).or_else(|| levels.last()).cloned()
    }
}

/// How deep a `style:parent-style-name` chain is followed before the file
/// is treated as circular.
const MAX_STYLE_DEPTH: usize = 32;

fn position(value: &str) -> Position {
    match value.split_whitespace().next().unwrap_or("") {
        "super" => Position::Super,
        "sub" => Position::Sub,
        // The other spelling is a percentage of the font size: positive
        // raises, negative lowers, and `0%` is the baseline.
        percent => match percent.trim_end_matches('%').parse::<f64>() {
            Ok(p) if p > 0.0 => Position::Super,
            Ok(p) if p < 0.0 => Position::Sub,
            _ => Position::Normal,
        },
    }
}

fn read_levels(style: &Node) -> Vec<Level> {
    let mut levels: Vec<Level> = Vec::new();
    for level in style.elems() {
        let number = match level.name.as_str() {
            "list-level-style-bullet" => None,
            "list-level-style-number" | "list-level-style-image" => {
                let format = level.attr("style:num-format").unwrap_or_default();
                // A numbered level with an empty format has no number to
                // show, and pandoc reads it as a bullet list.
                if format.is_empty() {
                    None
                } else {
                    Some(NumberFormat {
                        format: format.to_owned(),
                        prefix: level.attr("style:num-prefix").unwrap_or_default().to_owned(),
                        suffix: level.attr("style:num-suffix").unwrap_or_default().to_owned(),
                    })
                }
            }
            _ => continue,
        };
        let start = level
            .attr("text:start-value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        // `text:level` is 1-based and the levels arrive in order; placing
        // by index rather than pushing keeps a file that skips one honest.
        let index = level
            .attr("text:level")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|l| *l >= 1 && *l <= MAX_LIST_LEVELS)
            .map_or(levels.len(), |l| l - 1);
        if levels.len() <= index {
            levels.resize(index + 1, Level { number: None, start: 1 });
        }
        levels[index] = Level { number, start };
    }
    levels
}

/// How many list levels a style may declare. ODF's own limit is ten;
/// accepting an arbitrary `text:level` would let a one-line file ask for a
/// vector of four billion entries.
const MAX_LIST_LEVELS: usize = 64;

impl TextProps {
    /// Overlay `other` on top of these, as a nested span's style overlays
    /// its parent's: a property the inner style says nothing about keeps
    /// the outer answer.
    pub fn overlay(self, other: TextProps) -> TextProps {
        TextProps {
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            strike: other.strike.or(self.strike),
            position: other.position.or(self.position),
            fixed_pitch: other.fixed_pitch.or(self.fixed_pitch),
        }
    }

    /// Whether the run is strongly emphasized.
    pub fn is_bold(self) -> bool {
        self.bold.unwrap_or(false)
    }

    /// Whether the run is emphasized.
    ///
    /// Three properties land here, and only the first is obvious: italic,
    /// underline, and a *fixed-pitch font*. The last is pandoc's, measured
    /// rather than reasoned — a span in a font declared
    /// `style:font-pitch="fixed"` reads back as `Emph`, not as code.
    pub fn is_emph(self) -> bool {
        self.italic.unwrap_or(false)
            || self.underline.unwrap_or(false)
            || self.fixed_pitch.unwrap_or(false)
    }

    /// Whether the run is struck out.
    pub fn is_strikeout(self) -> bool {
        self.strike.unwrap_or(false)
    }

    /// Where the run sits relative to the baseline.
    pub fn position(self) -> Option<Position> {
        match self.position {
            Some(Position::Normal) | None => None,
            Some(position) => Some(position),
        }
    }
}
