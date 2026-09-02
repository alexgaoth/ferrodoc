//! TeX rendered as **inlines**, the way pandoc renders it.
//!
//! Pandoc does not write the TeX source for `commonmark`, `html` or
//! `plain`. It converts the expression to ordinary inlines — a variable
//! is emphasis, `^` is a superscript, `\alpha` is `α` — and lets each
//! writer render those: `$x^2$` is `x²` in plain text and
//! `<em>x</em><sup>2</sup>` in HTML, from the same two inlines.
//!
//! **Where the expression is more than symbols and scripts it gives up**
//! and writes the source between dollars. A fraction, a root, an
//! operator carrying limits: all of them come back out as `$\frac{1}{2}$`
//! from pandoc itself. [`tex_inlines`] returning `None` is that, and the
//! writers keep the fallback they already wrote.
//!
//! Every rule here is measured against the pinned pandoc, one expression
//! at a time, by `scripts/math.sh` — which is also what says how much of
//! the language this covers, rather than a claim in a comment.

use crate::Inline;

/// The space pandoc writes around a **binary operator**, four to the em.
const AROUND_BINARY: char = '\u{2005}';
/// The space it writes around a **relation**, three to the em — wider.
const AROUND_RELATION: char = '\u{2004}';
/// The space it writes **after** a comma or a semicolon, six to the em —
/// the narrowest of the three: `f(x,\u{2006}y)`.
const AFTER_PUNCTUATION: char = '\u{2006}';

/// What spacing a symbol takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A letter, a digit, a bracket: nothing around it.
    Ordinary,
    /// `+`, `×`: a four-per-em space on each side.
    Binary,
    /// `=`, `≤`, `→`: a three-per-em space on each side.
    Relation,
    /// `,`, `;`: a six-per-em space after it and nothing before.
    Punctuation,
}

/// One piece of an expression, before the spacing is decided.
struct Atom {
    inlines: Vec<Inline>,
    class: Class,
    /// Whether this is a `+` or `-` that would be a *sign* rather than an
    /// operator if nothing stood before it.
    signable: bool,
    /// Whether this is an operator that carries **limits** — `\sum` and
    /// the rest of the big ones. Pandoc renders `\sum_i^n` and gives up
    /// on `\sum_{i}^n`: both limits, and one of them a group, is where
    /// its own conversion stops. Measured; a rule nobody would guess.
    limits: bool,
}

/// Render TeX as the inlines pandoc renders it as, or `None` where
/// pandoc writes the source instead.
///
/// ```
/// # use ferrodoc_ast::{tex_inlines, Inline};
/// let inlines = tex_inlines("x^2").expect("a superscript renders");
/// assert!(matches!(inlines.as_slice(), [Inline::Emph(_), Inline::Superscript(_)]));
/// assert!(tex_inlines(r"\frac{1}{2}").is_none());
/// ```
#[must_use]
pub fn tex_inlines(tex: &str) -> Option<Vec<Inline>> {
    let mut parser = Tex { rest: tex };
    let mut atoms = Vec::new();
    while parser.skip_spaces() {
        atoms.push(parser.atom()?);
    }
    if atoms.is_empty() {
        return None;
    }
    Some(spaced(atoms))
}

/// Put the spacing in, once the neighbours are known.
///
/// A `+` or `-` with nothing but another operator before it is a **sign**
/// and takes no space at all: pandoc writes `a\u{2005}−\u{2005}b` and
/// `−x`.
fn spaced(atoms: Vec<Atom>) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut previous = Class::Binary; // nothing before the first atom
    for atom in atoms {
        let sign = atom.signable && previous != Class::Ordinary;
        let class = if sign { Class::Ordinary } else { atom.class };
        match class {
            Class::Ordinary => out.extend(atom.inlines),
            Class::Punctuation => {
                out.extend(atom.inlines);
                out.push(Inline::Str(AFTER_PUNCTUATION.to_string()));
            }
            Class::Binary | Class::Relation => {
                let space = if class == Class::Binary { AROUND_BINARY } else { AROUND_RELATION };
                out.push(Inline::Str(space.to_string()));
                out.extend(atom.inlines);
                out.push(Inline::Str(space.to_string()));
            }
        }
        previous = class;
    }
    out
}

struct Tex<'a> {
    rest: &'a str,
}

impl Tex<'_> {
    /// Skip the spaces TeX ignores; `false` at the end of the input.
    fn skip_spaces(&mut self) -> bool {
        self.rest = self.rest.trim_start_matches([' ', '\t', '\n']);
        !self.rest.is_empty()
    }

    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn take(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.rest = &self.rest[ch.len_utf8()..];
        Some(ch)
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.rest = &self.rest[ch.len_utf8()..];
            return true;
        }
        false
    }

    /// One atom and whatever scripts follow it.
    ///
    /// **Both a subscript and a superscript on the same atom is a
    /// fallback**: that is an operator with limits, which pandoc writes
    /// as the source — `$\sum_{i=1}^n$` comes back out of pandoc whole,
    /// where `\sum` alone is `∑`.
    fn atom(&mut self) -> Option<Atom> {
        let mut atom = self.unit()?;
        let mut scripts = Vec::new();
        let mut kinds = (false, false);
        let mut grouped = false;
        loop {
            self.rest = self.rest.trim_start_matches([' ', '\t', '\n']);
            let up = self.peek() == Some('^');
            let down = self.peek() == Some('_');
            if !up && !down {
                break;
            }
            // The same script twice is not an expression at all.
            if (up && kinds.0) || (down && kinds.1) {
                return None;
            }
            kinds = (kinds.0 || up, kinds.1 || down);
            self.take();
            self.skip_spaces();
            grouped |= self.peek() == Some('{');
            let inner = self.unit()?;
            let wrapped = if up {
                Inline::Superscript(inner.inlines)
            } else {
                Inline::Subscript(inner.inlines)
            };
            scripts.push(wrapped);
        }
        if atom.limits && kinds.0 && kinds.1 && grouped {
            return None;
        }
        atom.inlines.extend(scripts);
        Some(atom)
    }

    /// One unit: a group, a control sequence, or a single character.
    fn unit(&mut self) -> Option<Atom> {
        match self.peek()? {
            '{' => {
                self.take();
                let mut atoms = Vec::new();
                loop {
                    self.rest = self.rest.trim_start_matches([' ', '\t', '\n']);
                    if self.eat('}') {
                        break;
                    }
                    if self.rest.is_empty() {
                        return None;
                    }
                    atoms.push(self.atom()?);
                }
                Some(ordinary(spaced(atoms)))
            }
            '\\' => self.control(),
            // A script with nothing to attach to, a stray brace, an
            // alignment mark, a dollar: none of them is an expression
            // this writes, and pandoc writes the source for them too.
            '}' | '^' | '_' | '&' | '$' => None,
            _ => {
                let ch = self.take()?;
                Some(character(ch))
            }
        }
    }

    /// A control sequence: a symbol, a font, or a fallback.
    fn control(&mut self) -> Option<Atom> {
        self.take();
        let name: String = self.rest.chars().take_while(char::is_ascii_alphabetic).collect();
        if name.is_empty() {
            // `\%`, `\$`, `\{`, `\&`, `\#`, `\_`: the character itself.
            // `\\` is a line break, which no inline expression has.
            let ch = self.take()?;
            if matches!(ch, '%' | '$' | '{' | '}' | '&' | '#' | '_' | ' ' | ',' | ';' | '!') {
                return Some(if ch == ' ' || ch == ',' || ch == ';' || ch == '!' {
                    ordinary(vec![Inline::Str(" ".to_owned())])
                } else {
                    character(ch)
                });
            }
            return None;
        }
        self.rest = &self.rest[name.len()..];
        // `\left(` and `\right)` are the bracket and nothing else.
        if name == "left" || name == "right" {
            self.skip_spaces();
            if self.rest.starts_with('\\') {
                return self.control();
            }
            let ch = self.take()?;
            return Some(character(ch));
        }
        // An accent is the combining character for it, put after the
        // letter it sits on: `\hat{x}` is `x̂`, one character and a mark,
        // and it is still a variable.
        if let Some(mark) = accent_of(&name) {
            self.skip_spaces();
            if !self.eat('{') {
                return None;
            }
            let text: String = self.rest.chars().take_while(|c| *c != '}').collect();
            self.rest = &self.rest[text.len()..];
            if !self.eat('}') || text.is_empty() {
                return None;
            }
            let mut accented = String::new();
            for ch in text.chars() {
                accented.push(ch);
                accented.push(mark);
            }
            let letters = text.chars().all(char::is_alphabetic);
            let inlines = if letters {
                vec![Inline::Emph(vec![Inline::Str(accented)])]
            } else {
                vec![Inline::Str(accented)]
            };
            return Some(ordinary(inlines));
        }
        if let Some(font) = font_of(&name) {
            self.skip_spaces();
            if !self.eat('{') {
                return None;
            }
            let text: String = self.rest.chars().take_while(|c| *c != '}').collect();
            self.rest = &self.rest[text.len()..];
            if !self.eat('}') {
                return None;
            }
            return font(&text);
        }
        let (symbol, class) = SYMBOLS.iter().find(|(word, _, _)| *word == name).map(
            |(_, symbol, class)| (*symbol, *class),
        )?;
        // A Greek letter is a variable and set in italics; an operator is
        // not. Pandoc writes `<em>α</em>` and a bare `∑` — and a bare
        // `ℏ`, though it is a letter and `ℵ`, `ℓ`, `ℜ` and `ℑ` are set
        // in italics beside it. Measured one symbol at a time.
        if class == Class::Ordinary && symbol.is_alphabetic() && !UPRIGHT.contains(&symbol) {
            return Some(Atom {
                inlines: vec![Inline::Emph(vec![Inline::Str(symbol.to_string())])],
                class,
                signable: false,
                limits: false,
            });
        }
        Some(Atom {
            inlines: vec![Inline::Str(symbol.to_string())],
            class,
            signable: false,
            limits: BIG_OPERATORS.contains(&name.as_str()),
        })
    }
}

/// A single character of an expression: a letter is a variable and is set
/// in italics, a digit and a bracket are not.
fn character(ch: char) -> Atom {
    if ch.is_alphabetic() {
        return Atom {
            inlines: vec![Inline::Emph(vec![Inline::Str(ch.to_string())])],
            class: Class::Ordinary,
            signable: false,
            limits: false,
        };
    }
    let (text, class, signable) = match ch {
        '-' => ('\u{2212}', Class::Binary, true),
        '+' => ('+', Class::Binary, true),
        '*' => ('*', Class::Binary, false),
        '/' => ('/', Class::Ordinary, false),
        ',' => (',', Class::Punctuation, false),
        ';' => (';', Class::Punctuation, false),
        '=' => ('=', Class::Relation, false),
        '<' => ('<', Class::Relation, false),
        '>' => ('>', Class::Relation, false),
        other => (other, Class::Ordinary, false),
    };
    Atom { inlines: vec![Inline::Str(text.to_string())], class, signable, limits: false }
}

fn ordinary(inlines: Vec<Inline>) -> Atom {
    Atom { inlines, class: Class::Ordinary, signable: false, limits: false }
}

/// The combining mark an accent puts on the character under it.
fn accent_of(name: &str) -> Option<char> {
    match name {
        "hat" | "widehat" => Some('\u{0302}'),
        "bar" | "overline" => Some('\u{0304}'),
        "vec" => Some('\u{20D7}'),
        "tilde" | "widetilde" => Some('\u{0303}'),
        "dot" => Some('\u{0307}'),
        "acute" => Some('\u{0301}'),
        "grave" => Some('\u{0300}'),
        "check" => Some('\u{030C}'),
        "breve" => Some('\u{0306}'),
        _ => None,
    }
}

/// The fonts that are a **text** rather than a variable: `\text{hi}` is
/// `hi` and `\mathbf{x}` is bold, neither of them italic.
fn font_of(name: &str) -> Option<fn(&str) -> Option<Atom>> {
    match name {
        "text" | "textrm" | "mathrm" | "mbox" | "operatorname" => {
            Some(|text| Some(ordinary(vec![Inline::Str(text.to_owned())])))
        }
        "mathbf" | "textbf" | "boldsymbol" => Some(|text| {
            Some(ordinary(vec![Inline::Strong(vec![Inline::Str(text.to_owned())])]))
        }),
        "mathit" | "textit" | "mathnormal" => Some(|text| {
            Some(ordinary(vec![Inline::Emph(vec![Inline::Str(text.to_owned())])]))
        }),
        "mathbb" => Some(|text| Some(ordinary(vec![Inline::Str(mapped(text, blackboard)?)]))),
        "mathcal" => Some(|text| Some(ordinary(vec![Inline::Str(mapped(text, script)?)]))),
        _ => None,
    }
}

/// Every character through a map, or `None` where one has no answer.
fn mapped(text: &str, map: fn(char) -> Option<char>) -> Option<String> {
    text.chars().map(map).collect()
}

/// `\mathbb{R}` is `ℝ`. The double-struck letters are scattered across
/// two blocks: five of them are named exceptions in the BMP and the rest
/// are consecutive from U+1D538.
fn blackboard(ch: char) -> Option<char> {
    match ch {
        'C' => Some('\u{2102}'),
        'H' => Some('\u{210D}'),
        'N' => Some('\u{2115}'),
        'P' => Some('\u{2119}'),
        'Q' => Some('\u{211A}'),
        'R' => Some('\u{211D}'),
        'Z' => Some('\u{2124}'),
        'A'..='Z' => char::from_u32(0x1D538 + (ch as u32 - 'A' as u32)),
        'a'..='z' => char::from_u32(0x1D552 + (ch as u32 - 'a' as u32)),
        _ => None,
    }
}

/// `\mathcal{L}` is `ℒ`, with the same scattering.
fn script(ch: char) -> Option<char> {
    match ch {
        'B' => Some('\u{212C}'),
        'E' => Some('\u{2130}'),
        'F' => Some('\u{2131}'),
        'H' => Some('\u{210B}'),
        'I' => Some('\u{2110}'),
        'L' => Some('\u{2112}'),
        'M' => Some('\u{2133}'),
        'R' => Some('\u{211B}'),
        'e' => Some('\u{212F}'),
        'g' => Some('\u{210A}'),
        'o' => Some('\u{2134}'),
        'A'..='Z' => char::from_u32(0x1D49C + (ch as u32 - 'A' as u32)),
        'a'..='z' => char::from_u32(0x1D4B6 + (ch as u32 - 'a' as u32)),
        _ => None,
    }
}

/// The letters pandoc leaves upright, where it sets every other letter
/// of an expression in italics.
static UPRIGHT: &[char] = &['\u{210F}'];

/// The operators that carry **limits**, where pandoc's own conversion
/// stops as soon as both of them are written and either is a group.
static BIG_OPERATORS: &[&str] =
    &["sum", "prod", "int", "iint", "oint", "coprod", "bigcup", "bigcap"];

/// The control sequences this renders, with the spacing each takes.
///
/// Anything not here is a fallback, which is how a fraction, a root and
/// an environment leave as the source they came in as.
static SYMBOLS: &[(&str, char, Class)] = &[
    // Greek, lower case
    ("alpha", 'α', Class::Ordinary), ("beta", 'β', Class::Ordinary),
    ("gamma", 'γ', Class::Ordinary), ("delta", 'δ', Class::Ordinary),
    ("epsilon", 'ϵ', Class::Ordinary), ("varepsilon", 'ε', Class::Ordinary),
    ("zeta", 'ζ', Class::Ordinary), ("eta", 'η', Class::Ordinary),
    ("theta", 'θ', Class::Ordinary), ("vartheta", 'ϑ', Class::Ordinary),
    ("iota", 'ι', Class::Ordinary), ("kappa", 'κ', Class::Ordinary),
    ("lambda", 'λ', Class::Ordinary), ("mu", 'μ', Class::Ordinary),
    ("nu", 'ν', Class::Ordinary), ("xi", 'ξ', Class::Ordinary),
    ("pi", 'π', Class::Ordinary), ("varpi", 'ϖ', Class::Ordinary),
    ("rho", 'ρ', Class::Ordinary), ("varrho", 'ϱ', Class::Ordinary),
    ("sigma", 'σ', Class::Ordinary), ("varsigma", 'ς', Class::Ordinary),
    ("tau", 'τ', Class::Ordinary), ("upsilon", 'υ', Class::Ordinary),
    ("phi", 'ϕ', Class::Ordinary), ("varphi", 'φ', Class::Ordinary),
    ("chi", 'χ', Class::Ordinary), ("psi", 'ψ', Class::Ordinary),
    ("omega", 'ω', Class::Ordinary),
    // Greek, upper case
    ("Gamma", 'Γ', Class::Ordinary), ("Delta", 'Δ', Class::Ordinary),
    ("Theta", 'Θ', Class::Ordinary), ("Lambda", 'Λ', Class::Ordinary),
    ("Xi", 'Ξ', Class::Ordinary), ("Pi", 'Π', Class::Ordinary),
    ("Sigma", 'Σ', Class::Ordinary), ("Upsilon", 'Υ', Class::Ordinary),
    ("Phi", 'Φ', Class::Ordinary), ("Psi", 'Ψ', Class::Ordinary),
    ("Omega", 'Ω', Class::Ordinary),
    // Binary operators
    ("times", '×', Class::Binary), ("div", '÷', Class::Binary),
    ("pm", '±', Class::Binary), ("mp", '∓', Class::Binary),
    ("cdot", '⋅', Class::Binary), ("ast", '∗', Class::Binary),
    ("star", '⋆', Class::Binary), ("circ", '∘', Class::Binary),
    ("bullet", '∙', Class::Binary), ("cup", '∪', Class::Binary),
    ("cap", '∩', Class::Binary), ("vee", '∨', Class::Binary),
    ("wedge", '∧', Class::Binary), ("oplus", '⊕', Class::Binary),
    ("otimes", '⊗', Class::Binary), ("setminus", '∖', Class::Binary),
    // Relations
    ("leq", '≤', Class::Relation), ("le", '≤', Class::Relation),
    ("geq", '≥', Class::Relation), ("ge", '≥', Class::Relation),
    ("neq", '≠', Class::Relation), ("ne", '≠', Class::Relation),
    ("approx", '≈', Class::Relation), ("equiv", '≡', Class::Relation),
    ("sim", '∼', Class::Relation), ("simeq", '≃', Class::Relation),
    ("cong", '≅', Class::Relation), ("propto", '∝', Class::Relation),
    ("in", '∈', Class::Relation), ("notin", '∉', Class::Relation),
    ("ni", '∋', Class::Relation), ("subset", '⊂', Class::Relation),
    ("supset", '⊃', Class::Relation), ("subseteq", '⊆', Class::Relation),
    ("supseteq", '⊇', Class::Relation), ("ll", '≪', Class::Relation),
    ("gg", '≫', Class::Relation), ("perp", '⊥', Class::Relation),
    ("parallel", '∥', Class::Relation), ("mid", '∣', Class::Relation),
    // Arrows, which are relations too
    ("to", '→', Class::Relation), ("rightarrow", '→', Class::Relation),
    ("leftarrow", '←', Class::Relation), ("gets", '←', Class::Relation),
    ("leftrightarrow", '↔', Class::Relation),
    ("Rightarrow", '⇒', Class::Relation), ("Leftarrow", '⇐', Class::Relation),
    ("Leftrightarrow", '⇔', Class::Relation), ("mapsto", '↦', Class::Relation),
    ("implies", '⟹', Class::Relation), ("iff", '⟺', Class::Relation),
    // Ordinary symbols
    ("infty", '∞', Class::Ordinary), ("partial", '∂', Class::Ordinary),
    ("nabla", '∇', Class::Ordinary), ("forall", '∀', Class::Ordinary),
    ("exists", '∃', Class::Ordinary), ("neg", '¬', Class::Ordinary),
    ("emptyset", '∅', Class::Ordinary), ("varnothing", '∅', Class::Ordinary),
    ("aleph", 'ℵ', Class::Ordinary), ("hbar", 'ℏ', Class::Ordinary),
    ("ell", 'ℓ', Class::Ordinary), ("Re", 'ℜ', Class::Ordinary),
    ("Im", 'ℑ', Class::Ordinary), ("prime", '′', Class::Ordinary),
    ("degree", '°', Class::Ordinary), ("angle", '∠', Class::Ordinary),
    ("sum", '∑', Class::Ordinary), ("prod", '∏', Class::Ordinary),
    ("int", '∫', Class::Ordinary), ("iint", '∬', Class::Ordinary),
    ("oint", '∮', Class::Ordinary), ("surd", '√', Class::Ordinary),
    ("dots", '…', Class::Ordinary), ("ldots", '…', Class::Ordinary),
    ("cdots", '⋯', Class::Ordinary), ("vdots", '⋮', Class::Ordinary),
    ("ddots", '⋱', Class::Ordinary), ("therefore", '∴', Class::Ordinary),
    ("because", '∵', Class::Ordinary), ("langle", '⟨', Class::Ordinary),
    ("rangle", '⟩', Class::Ordinary), ("lfloor", '⌊', Class::Ordinary),
    ("rfloor", '⌋', Class::Ordinary), ("lceil", '⌈', Class::Ordinary),
    ("rceil", '⌉', Class::Ordinary),
];
