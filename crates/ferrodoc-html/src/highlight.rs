//! Syntax highlighting, in the shape pandoc's `skylighting` emits.
//!
//! **What this is not** is a port of skylighting: that reads KDE's syntax
//! XML, and reproducing one of those state machines by hand is how a
//! highlighter comes to be plausibly wrong. What it is instead is the
//! method this repository uses everywhere else — every table below was
//! read off `pandoc -f commonmark -t html` rather than remembered, and
//! `scripts/highlight.sh` holds the result to real source files, not to
//! fixtures written to pass it.
//!
//! **A language this does not know degrades to exactly what the writer
//! emits without highlighting**, which is what makes a short list safe:
//! `<pre class="whatever"><code>`, byte for byte as before.

use std::fmt::Write as _;

/// A token class, spelled as skylighting spells it in `class`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Class {
    /// Not a token: ordinary text, written without a span.
    Normal,
    Keyword,
    ControlFlow,
    DataType,
    DecVal,
    BaseN,
    Float,
    BuiltIn,
    Str,
    Char,
    SpecialChar,
    Comment,
    Operator,
    Preprocessor,
    Import,
}

impl Class {
    fn name(self) -> Option<&'static str> {
        Some(match self {
            Class::Normal => return None,
            Class::Keyword => "kw",
            Class::ControlFlow => "cf",
            Class::DataType => "dt",
            Class::DecVal => "dv",
            Class::BaseN => "bn",
            Class::Float => "fl",
            Class::BuiltIn => "bu",
            Class::Str => "st",
            Class::Char => "ch",
            Class::SpecialChar => "sc",
            Class::Comment => "co",
            Class::Operator => "op",
            Class::Preprocessor => "pp",
            Class::Import => "im",
        })
    }
}

/// One language's rules. Every field was measured against pandoc.
struct Syntax {
    /// What `<code class="sourceCode …">` says, whatever the fence wrote.
    canonical: &'static str,
    /// Every name pandoc accepts for it, canonical first.
    names: &'static [&'static str],
    /// Sorted by word, so the lookup is a binary search.
    keywords: &'static [(&'static str, Class)],
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Quote character and the class the quoted run takes.
    quotes: &'static [(char, Class)],
    /// `#include` and friends: the line is `pp`, and a `<…>` after an
    /// include is `im`.
    preprocessor: bool,
    /// `printf`'s `%s` inside a string is a `sc`, as an escape is.
    format_specifiers: bool,
}

const OPERATORS: &str = "+-*/%&|^~<>!=?:;,.()[]{}";

static C: Syntax = Syntax {
    canonical: "c",
    names: &["c"],
    // Probed one word at a time: `NULL`, `printf` and `malloc` are
    // **not** classed, which is the sort of thing a list written from
    // memory gets wrong in the safe-looking direction.
    keywords: &[
        ("FILE", Class::DataType),
        ("_Bool", Class::DataType),
        ("_Complex", Class::DataType),
        ("_Imaginary", Class::DataType),
        ("alignas", Class::Keyword),
        ("alignof", Class::Keyword),
        ("auto", Class::Keyword),
        ("bool", Class::DataType),
        ("break", Class::ControlFlow),
        ("case", Class::ControlFlow),
        ("char", Class::DataType),
        ("const", Class::DataType),
        ("continue", Class::ControlFlow),
        ("default", Class::ControlFlow),
        ("do", Class::ControlFlow),
        ("double", Class::DataType),
        ("else", Class::ControlFlow),
        ("enum", Class::Keyword),
        ("extern", Class::Keyword),
        ("false", Class::Keyword),
        ("float", Class::DataType),
        ("for", Class::ControlFlow),
        ("goto", Class::ControlFlow),
        ("if", Class::ControlFlow),
        ("inline", Class::Keyword),
        ("int", Class::DataType),
        ("int32_t", Class::DataType),
        ("long", Class::DataType),
        ("nullptr", Class::Keyword),
        ("register", Class::DataType),
        ("restrict", Class::DataType),
        ("return", Class::ControlFlow),
        ("short", Class::DataType),
        ("signed", Class::DataType),
        ("size_t", Class::DataType),
        ("sizeof", Class::Keyword),
        ("static", Class::DataType),
        ("static_assert", Class::Keyword),
        ("struct", Class::Keyword),
        ("switch", Class::ControlFlow),
        ("thread_local", Class::DataType),
        ("true", Class::Keyword),
        ("typedef", Class::Keyword),
        ("typeof", Class::Keyword),
        ("uint8_t", Class::DataType),
        ("union", Class::Keyword),
        ("unsigned", Class::DataType),
        ("void", Class::DataType),
        ("volatile", Class::DataType),
        ("while", Class::ControlFlow),
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &[('"', Class::Str), ('\'', Class::Char)],
    preprocessor: true,
    format_specifiers: true,
};

static SYNTAXES: &[&Syntax] = &[&C];

/// The syntax a fence's language name asks for, if this knows it.
fn syntax(name: &str) -> Option<&'static Syntax> {
    let lowered = name.to_ascii_lowercase();
    SYNTAXES
        .iter()
        .copied()
        .find(|syntax| syntax.names.contains(&lowered.as_str()))
}

/// Whether a code block written in `name` would be highlighted.
pub(crate) fn known(name: &str) -> bool {
    syntax(name).is_some()
}

/// What `<code class="sourceCode …">` says for a block written `name`.
pub(crate) fn canonical(name: &str) -> &'static str {
    syntax(name).map_or("", |syntax| syntax.canonical)
}

/// Whether the scanner is inside a block comment when the next line
/// starts. A run of code is highlighted line by line, so the state has
/// to survive between them.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct State {
    in_block_comment: bool,
}

/// One line, as a run of `(class, text)` pieces with adjacent pieces of
/// the same class already merged — pandoc emits one span per run.
pub(crate) fn line(text: &str, name: &str, state: &mut State) -> Vec<(Class, String)> {
    let Some(syntax) = syntax(name) else {
        return vec![(Class::Normal, text.to_owned())];
    };
    let mut out: Vec<(Class, String)> = Vec::new();
    let mut at = 0;
    if state.in_block_comment {
        let (_, close) = syntax.block_comment.expect("only set inside one");
        match text.find(close) {
            None => return vec![(Class::Comment, text.to_owned())],
            Some(end) => {
                push(&mut out, Class::Comment, &text[..end + close.len()]);
                at = end + close.len();
                state.in_block_comment = false;
            }
        }
    } else if syntax.preprocessor {
        at = directive(text, syntax, &mut out);
    }
    scan(text, at, syntax, state, &mut out);
    out
}

/// A `#…` line: the directive is `pp`, an include's target is `im`, and
/// a comment ends the directive rather than being part of it. `#define`
/// is the exception that keeps tokenizing — measured, and the reason
/// this is not simply "the whole line".
fn directive(text: &str, syntax: &Syntax, out: &mut Vec<(Class, String)>) -> usize {
    if text.trim_start().as_bytes().first() != Some(&b'#') {
        return 0;
    }
    let name: String = text
        .trim_start()
        .trim_start_matches('#')
        .trim_start()
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    if name == "include" {
        let target = text.find(['<', '"']);
        let Some(start) = target else {
            push(out, Class::Preprocessor, text);
            return text.len();
        };
        let close = if text.as_bytes()[start] == b'<' { '>' } else { '"' };
        let end = text[start + 1..].find(close).map_or(text.len(), |i| start + 1 + i + 1);
        push(out, Class::Preprocessor, &text[..start]);
        push(out, Class::Import, &text[start..end]);
        return end;
    }
    if name == "define" {
        // The directive and the name it defines; the value tokenizes.
        let after = text.trim_start().trim_start_matches('#').trim_start();
        let indent = text.len() - after.len();
        let word = after["define".len()..].trim_start();
        let named: usize = word.chars().take_while(|c| c.is_alphanumeric() || *c == '_').count();
        let end = indent + "define".len() + (after["define".len()..].len() - word.len()) + named;
        let end = text[end..].find(|c: char| !c.is_whitespace()).map_or(text.len(), |i| end + i);
        push(out, Class::Preprocessor, &text[..end]);
        return end;
    }
    // Anything else runs to the end of the line, or to a comment.
    let stop = [syntax.block_comment.map(|(open, _)| open), syntax.line_comment.first().copied()]
        .into_iter()
        .flatten()
        .filter_map(|open| text.find(open))
        .min()
        .unwrap_or(text.len());
    push(out, Class::Preprocessor, &text[..stop]);
    stop
}

fn scan(text: &str, from: usize, syntax: &Syntax, state: &mut State, out: &mut Vec<(Class, String)>) {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < text.len() {
        let rest = &text[at..];
        if let Some(open) = syntax.line_comment.iter().find(|open| rest.starts_with(**open)) {
            let _ = open;
            push(out, Class::Comment, rest);
            return;
        }
        if let Some((open, close)) = syntax.block_comment
            && rest.starts_with(open)
        {
            match rest[open.len()..].find(close) {
                None => {
                    push(out, Class::Comment, rest);
                    state.in_block_comment = true;
                    return;
                }
                Some(end) => {
                    let end = open.len() + end + close.len();
                    push(out, Class::Comment, &rest[..end]);
                    at += end;
                    continue;
                }
            }
        }
        let quote = rest.chars().next().and_then(|first| {
            syntax.quotes.iter().find(|(q, _)| *q == first).copied()
        });
        if let Some((quote, class)) = quote {
            at = quoted(text, at, quote, class, syntax, out);
            continue;
        }
        let byte = bytes[at];
        if byte.is_ascii_digit() {
            at = number(text, at, out);
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let class = syntax
                .keywords
                .binary_search_by_key(&word.as_str(), |(name, _)| name)
                .map_or(Class::Normal, |index| syntax.keywords[index].1);
            push(out, class, &word);
            at += word.len();
            continue;
        }
        if OPERATORS.contains(char::from(byte)) {
            let run = rest.find(|c: char| !OPERATORS.contains(c)).unwrap_or(rest.len());
            push(out, Class::Operator, &rest[..run]);
            at += run;
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        push(out, Class::Normal, &rest[..width]);
        at += width;
    }
}

/// A quoted run: the quotes and the text are the run's own class, an
/// escape sequence is `sc`, and so is a `printf` conversion.
fn quoted(
    text: &str,
    from: usize,
    quote: char,
    class: Class,
    syntax: &Syntax,
    out: &mut Vec<(Class, String)>,
) -> usize {
    push(out, class, &text[from..from + quote.len_utf8()]);
    let mut at = from + quote.len_utf8();
    while at < text.len() {
        let rest = &text[at..];
        if syntax.format_specifiers && class == Class::Str
            && let Some(len) = specifier(rest)
        {
            push(out, Class::SpecialChar, &rest[..len]);
            at += len;
            continue;
        }
        if rest.starts_with('\\') {
            let mut end = 0;
            while rest[end..].starts_with('\\') {
                end = (end + 2).min(rest.len());
            }
            push(out, Class::SpecialChar, &rest[..end]);
            at += end;
            continue;
        }
        if rest.starts_with(quote) {
            push(out, class, &rest[..quote.len_utf8()]);
            return at + quote.len_utf8();
        }
        let stop = rest
            .char_indices()
            .find(|(index, c)| {
                *index > 0
                    && (*c == quote || *c == '\\' || (syntax.format_specifiers && *c == '%'))
            })
            .map_or(rest.len(), |(index, _)| index);
        push(out, class, &rest[..stop]);
        at += stop;
    }
    at
}

/// The length of a `printf` conversion at the front of `rest`.
fn specifier(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices();
    if chars.next()?.1 != '%' {
        return None;
    }
    let mut end = None;
    for (index, c) in chars {
        if "-+ #0123456789.*hlLqjzt".contains(c) {
            continue;
        }
        if "diouxXeEfgGcspn%".contains(c) {
            end = Some(index + c.len_utf8());
        }
        break;
    }
    end
}

/// A numeric literal, and the suffix letters after it, which pandoc
/// classes `bu` rather than folding into the number.
fn number(text: &str, from: usize, out: &mut Vec<(Class, String)>) -> usize {
    let rest = &text[from..];
    let based = ["0x", "0X", "0b", "0B"]
        .iter()
        .find(|prefix| rest.starts_with(**prefix))
        .map(|prefix| {
            prefix.len()
                + rest[prefix.len()..]
                    .find(|c: char| !c.is_ascii_hexdigit())
                    .unwrap_or(rest.len() - prefix.len())
        });
    let mut at = if let Some(end) = based {
        push(out, Class::BaseN, &rest[..end]);
        end
    } else {
        {
            let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            let after = &rest[digits..];
            let fraction = after.starts_with('.')
                && after[1..].starts_with(|c: char| c.is_ascii_digit() || c == 'e' || c == 'E');
            let exponent = after.starts_with(['e', 'E']);
            if fraction || exponent {
                let mut end = digits + usize::from(fraction);
                end += rest[end..].find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len() - end);
                if rest[end..].starts_with(['e', 'E']) {
                    let mut exp = end + 1;
                    if rest[exp..].starts_with(['+', '-']) {
                        exp += 1;
                    }
                    exp += rest[exp..].find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len() - exp);
                    end = exp;
                }
                push(out, Class::Float, &rest[..end]);
                end
            } else if rest.starts_with('0') && digits > 1 {
                push(out, Class::BaseN, &rest[..digits]);
                digits
            } else {
                push(out, Class::DecVal, &rest[..digits]);
                digits
            }
        }
    };
    let suffix = rest[at..].find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len() - at);
    if suffix > 0 {
        push(out, Class::BuiltIn, &rest[at..at + suffix]);
        at += suffix;
    }
    from + at
}

/// Add a piece, merging it into the one before when the class is the
/// same — pandoc writes one span per run, not one per token.
fn push(out: &mut Vec<(Class, String)>, class: Class, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some((last, run)) if *last == class => run.push_str(text),
        _ => out.push((class, text.to_owned())),
    }
}

/// Render one line's pieces, escaping the text as HTML.
pub(crate) fn write_line(out: &mut String, pieces: &[(Class, String)], escape: fn(&mut String, &str)) {
    for (class, text) in pieces {
        match class.name() {
            None => escape(out, text),
            Some(name) => {
                let _ = write!(out, "<span class=\"{name}\">");
                escape(out, text);
                out.push_str("</span>");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Class, State, line};

    fn classes(code: &str, language: &str) -> Vec<(Class, String)> {
        let mut state = State::default();
        line(code, language, &mut state)
    }

    /// Every expectation is off `pandoc -f commonmark -t html`, and none
    /// of them is a rule anyone would write from memory. `NULL`,
    /// `printf` and `malloc` are **not** classed, which is exactly the
    /// sort of thing a keyword list written from memory gets wrong in
    /// the safe-looking direction.
    #[test]
    fn c_is_tokenized_the_way_skylighting_tokenizes_it() {
        let text = |pieces: &[(Class, &str)]| {
            pieces.iter().map(|(c, t)| (*c, (*t).to_owned())).collect::<Vec<_>>()
        };
        assert_eq!(
            classes("int x = 33;", "c"),
            text(&[
                (Class::DataType, "int"),
                (Class::Normal, " x "),
                (Class::Operator, "="),
                (Class::Normal, " "),
                (Class::DecVal, "33"),
                (Class::Operator, ";"),
            ])
        );
        // Adjacent operator characters are **one** span.
        assert_eq!(
            classes("f(a);", "c"),
            text(&[
                (Class::Normal, "f"),
                (Class::Operator, "("),
                (Class::Normal, "a"),
                (Class::Operator, ");"),
            ])
        );
        // Numbers: base, float, and the suffix as a `bu`.
        assert_eq!(
            classes("0x1f 017 1.5e3 42u", "c"),
            text(&[
                (Class::BaseN, "0x1f"),
                (Class::Normal, " "),
                (Class::BaseN, "017"),
                (Class::Normal, " "),
                (Class::Float, "1.5e3"),
                (Class::Normal, " "),
                (Class::DecVal, "42"),
                (Class::BuiltIn, "u"),
            ])
        );
        // An escape and a `printf` conversion are both `sc`, and they
        // merge with each other when adjacent.
        assert_eq!(
            classes("\"a %s\\n\"", "c"),
            text(&[
                (Class::Str, "\"a "),
                (Class::SpecialChar, "%s\\n"),
                (Class::Str, "\""),
            ])
        );
        // `#include` splits into the directive and its target.
        assert_eq!(
            classes("#include <stdio.h>", "c"),
            text(&[(Class::Preprocessor, "#include "), (Class::Import, "<stdio.h>")])
        );
        // A comment ends a directive rather than being part of it.
        assert_eq!(
            classes("#endif /* X */", "c"),
            text(&[(Class::Preprocessor, "#endif "), (Class::Comment, "/* X */")])
        );
    }

    /// A block comment survives between lines, because a run of code is
    /// highlighted one line at a time.
    #[test]
    fn a_block_comment_carries_to_the_next_line() {
        let mut state = State::default();
        assert_eq!(line("/* one", "c", &mut state), vec![(Class::Comment, "/* one".to_owned())]);
        assert_eq!(line(" two */ x", "c", &mut state)[0], (Class::Comment, " two */".to_owned()));
        assert!(!state.in_block_comment);
    }

    /// A language this does not know is one piece of ordinary text, so
    /// the writer emits what it always emitted.
    #[test]
    fn an_unknown_language_is_left_alone() {
        assert_eq!(classes("int x;", "aaa"), vec![(Class::Normal, "int x;".to_owned())]);
        assert!(!super::known("aaa"));
        assert!(super::known("c"));
        // Matching is case-insensitive; the canonical name is not the
        // one written.
        assert!(super::known("C"));
        assert_eq!(super::canonical("C"), "c");
    }
}
