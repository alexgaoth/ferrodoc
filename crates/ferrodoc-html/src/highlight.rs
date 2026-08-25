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
    Variable,
    Attribute,
    SpecialString,
    Function,
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
            Class::Variable => "va",
            Class::Attribute => "at",
            Class::SpecialString => "ss",
            Class::Function => "fu",
        })
    }
}

/// One language's rules. Every field was measured against pandoc, and
/// several are the opposite of the obvious guess — see each comment.
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
    /// The quirks this language has, as named bits — a struct of five
    /// booleans is a struct nobody reads correctly at a call site.
    quirks: u8,
    /// The characters that make an operator span. **Not the same set in
    /// every language**: python leaves `,`, `.`, brackets and `:` plain,
    /// where C makes every one of them an `op`.
    operators: &'static str,
    /// Letters that may sit in front of a quote (`r"…"`, `b"…"`), and
    /// `f`, which makes the run a `ss` with its braces `sc`.
    string_prefixes: &'static str,
    /// A digit separator that is part of the number (`1_000`).
    digit_separator: Option<char>,
    /// What an escape sequence inside a string is classed. **Not the
    /// same in both**: C says `sc`, python says `ch`.
    escape: Class,
}

/// `#include` and friends: the line is `pp`, and a `<…>` after an include
/// is `im`.
const PREPROCESSOR: u8 = 1 << 0;
/// A `printf` conversion inside a string is a `sc`, as an escape is.
const CONVERSIONS: u8 = 1 << 1;
/// A string that is the first thing on its line, at bracket depth zero,
/// is a **comment** — python's docstrings. Measured: `x = "a"` is a `st`,
/// a bare `"a"` on its own line is a `co`, and `"a"` inside a `[` run is
/// a `st` again.
const DOCSTRINGS: u8 = 1 << 2;
/// `@name` at the start of a line is an `at`.
const DECORATORS: u8 = 1 << 3;
/// `{…}` inside an ordinary string is one `sc` — python's `str.format`
/// placeholders. In an f-string it is the braces alone that are `sc`,
/// with code between them, which is a different rule.
const PLACEHOLDERS: u8 = 1 << 4;

impl Syntax {
    fn has(&self, quirk: u8) -> bool {
        self.quirks & quirk != 0
    }
}

const C_OPERATORS: &str = "+-*/%&|^~<>!=?:;,.()[]{}";
/// Python's set, which is C's without the brackets, the comma, the dot,
/// the colon and the question mark — all of those are plain text there.
const PYTHON_OPERATORS: &str = "+-*/%&|^~<>!=;";

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
    quirks: PREPROCESSOR | CONVERSIONS,
    operators: C_OPERATORS,
    string_prefixes: "",
    digit_separator: None,
    escape: Class::SpecialChar,
};

static PYTHON: Syntax = Syntax {
    canonical: "python",
    names: &["python", "python3", "py"],
    // **Probed over python's own vocabulary**, not over a list written
    // from memory: `dir(builtins)` plus `keyword.kwlist` plus the names
    // Python 2 had, 211 of them, each read back from pandoc. Choosing
    // the probe set by hand is how `file` came to be missing on the
    // first attempt, and the only reason that was caught is that a real
    // file used it.
    //
    // The shape of the answer is worth seeing: 67 of them are `pp` (every
    // exception and warning), 86 are `bu`, and `self` sits with `True`
    // and `None` as a `va`.
    keywords: &[
        ("ArithmeticError", Class::Preprocessor),
        ("AssertionError", Class::Preprocessor),
        ("AttributeError", Class::Preprocessor),
        ("BaseException", Class::Preprocessor),
        ("BlockingIOError", Class::Preprocessor),
        ("BrokenPipeError", Class::Preprocessor),
        ("BufferError", Class::Preprocessor),
        ("BytesWarning", Class::Preprocessor),
        ("ChildProcessError", Class::Preprocessor),
        ("ConnectionAbortedError", Class::Preprocessor),
        ("ConnectionError", Class::Preprocessor),
        ("ConnectionRefusedError", Class::Preprocessor),
        ("ConnectionResetError", Class::Preprocessor),
        ("DeprecationWarning", Class::Preprocessor),
        ("EOFError", Class::Preprocessor),
        ("Ellipsis", Class::Variable),
        ("EncodingWarning", Class::Preprocessor),
        ("EnvironmentError", Class::Preprocessor),
        ("Exception", Class::Preprocessor),
        ("False", Class::Variable),
        ("FileExistsError", Class::Preprocessor),
        ("FileNotFoundError", Class::Preprocessor),
        ("FloatingPointError", Class::Preprocessor),
        ("FutureWarning", Class::Preprocessor),
        ("GeneratorExit", Class::Preprocessor),
        ("IOError", Class::Preprocessor),
        ("ImportError", Class::Preprocessor),
        ("ImportWarning", Class::Preprocessor),
        ("IndentationError", Class::Preprocessor),
        ("IndexError", Class::Preprocessor),
        ("InterruptedError", Class::Preprocessor),
        ("IsADirectoryError", Class::Preprocessor),
        ("KeyError", Class::Preprocessor),
        ("KeyboardInterrupt", Class::Preprocessor),
        ("LookupError", Class::Preprocessor),
        ("MemoryError", Class::Preprocessor),
        ("ModuleNotFoundError", Class::Preprocessor),
        ("NameError", Class::Preprocessor),
        ("None", Class::Variable),
        ("NotADirectoryError", Class::Preprocessor),
        ("NotImplemented", Class::Variable),
        ("NotImplementedError", Class::Preprocessor),
        ("OSError", Class::Preprocessor),
        ("OverflowError", Class::Preprocessor),
        ("PendingDeprecationWarning", Class::Preprocessor),
        ("PermissionError", Class::Preprocessor),
        ("ProcessLookupError", Class::Preprocessor),
        ("RecursionError", Class::Preprocessor),
        ("ReferenceError", Class::Preprocessor),
        ("ResourceWarning", Class::Preprocessor),
        ("RuntimeError", Class::Preprocessor),
        ("RuntimeWarning", Class::Preprocessor),
        ("StopAsyncIteration", Class::Preprocessor),
        ("StopIteration", Class::Preprocessor),
        ("SyntaxError", Class::Preprocessor),
        ("SyntaxWarning", Class::Preprocessor),
        ("SystemError", Class::Preprocessor),
        ("SystemExit", Class::Preprocessor),
        ("TabError", Class::Preprocessor),
        ("TimeoutError", Class::Preprocessor),
        ("True", Class::Variable),
        ("TypeError", Class::Preprocessor),
        ("UnboundLocalError", Class::Preprocessor),
        ("UnicodeDecodeError", Class::Preprocessor),
        ("UnicodeEncodeError", Class::Preprocessor),
        ("UnicodeError", Class::Preprocessor),
        ("UnicodeTranslateError", Class::Preprocessor),
        ("UnicodeWarning", Class::Preprocessor),
        ("UserWarning", Class::Preprocessor),
        ("ValueError", Class::Preprocessor),
        ("Warning", Class::Preprocessor),
        ("ZeroDivisionError", Class::Preprocessor),
        ("__all__", Class::Variable),
        ("__class__", Class::Variable),
        ("__debug__", Class::Variable),
        ("__dir__", Class::Function),
        ("__file__", Class::Variable),
        ("__format__", Class::Function),
        ("__init__", Class::Function),
        ("__qualname__", Class::Variable),
        ("__slots__", Class::Variable),
        ("__import__", Class::BuiltIn),
        ("__name__", Class::Variable),
        ("abs", Class::BuiltIn),
        ("aiter", Class::BuiltIn),
        ("all", Class::BuiltIn),
        ("and", Class::Keyword),
        ("anext", Class::BuiltIn),
        ("any", Class::BuiltIn),
        ("apply", Class::BuiltIn),
        ("as", Class::Import),
        ("ascii", Class::BuiltIn),
        ("assert", Class::ControlFlow),
        ("async", Class::ControlFlow),
        ("await", Class::ControlFlow),
        ("basestring", Class::BuiltIn),
        ("bin", Class::BuiltIn),
        ("bool", Class::BuiltIn),
        ("break", Class::ControlFlow),
        ("breakpoint", Class::BuiltIn),
        ("buffer", Class::BuiltIn),
        ("bytearray", Class::BuiltIn),
        ("bytes", Class::BuiltIn),
        ("callable", Class::BuiltIn),
        ("chr", Class::BuiltIn),
        ("class", Class::Keyword),
        ("classmethod", Class::BuiltIn),
        ("cmp", Class::BuiltIn),
        ("coerce", Class::BuiltIn),
        ("compile", Class::BuiltIn),
        ("complex", Class::BuiltIn),
        ("continue", Class::ControlFlow),
        ("def", Class::Keyword),
        ("del", Class::Keyword),
        ("delattr", Class::BuiltIn),
        ("dict", Class::BuiltIn),
        ("dir", Class::BuiltIn),
        ("divmod", Class::BuiltIn),
        ("elif", Class::ControlFlow),
        ("else", Class::ControlFlow),
        ("enumerate", Class::BuiltIn),
        ("eval", Class::BuiltIn),
        ("except", Class::ControlFlow),
        ("exec", Class::BuiltIn),
        ("execfile", Class::BuiltIn),
        ("file", Class::BuiltIn),
        ("filter", Class::BuiltIn),
        ("finally", Class::ControlFlow),
        ("float", Class::BuiltIn),
        ("for", Class::ControlFlow),
        ("format", Class::BuiltIn),
        ("from", Class::Import),
        ("frozenset", Class::BuiltIn),
        ("getattr", Class::BuiltIn),
        ("global", Class::Keyword),
        ("globals", Class::BuiltIn),
        ("hasattr", Class::BuiltIn),
        ("hash", Class::BuiltIn),
        ("help", Class::BuiltIn),
        ("hex", Class::BuiltIn),
        ("id", Class::BuiltIn),
        ("if", Class::ControlFlow),
        ("import", Class::Import),
        ("in", Class::Keyword),
        ("input", Class::BuiltIn),
        ("int", Class::BuiltIn),
        ("intern", Class::BuiltIn),
        ("is", Class::Keyword),
        ("isinstance", Class::BuiltIn),
        ("issubclass", Class::BuiltIn),
        ("iter", Class::BuiltIn),
        ("lambda", Class::Keyword),
        ("len", Class::BuiltIn),
        ("list", Class::BuiltIn),
        ("locals", Class::BuiltIn),
        ("long", Class::BuiltIn),
        ("map", Class::BuiltIn),
        ("max", Class::BuiltIn),
        ("memoryview", Class::BuiltIn),
        ("min", Class::BuiltIn),
        ("next", Class::BuiltIn),
        ("nonlocal", Class::Keyword),
        ("not", Class::Keyword),
        ("object", Class::BuiltIn),
        ("oct", Class::BuiltIn),
        ("open", Class::BuiltIn),
        ("or", Class::Keyword),
        ("ord", Class::BuiltIn),
        ("pass", Class::ControlFlow),
        ("pow", Class::BuiltIn),
        ("print", Class::BuiltIn),
        ("property", Class::BuiltIn),
        ("raise", Class::ControlFlow),
        ("range", Class::BuiltIn),
        ("raw_input", Class::BuiltIn),
        ("reduce", Class::BuiltIn),
        ("reload", Class::BuiltIn),
        ("repr", Class::BuiltIn),
        ("return", Class::ControlFlow),
        ("reversed", Class::BuiltIn),
        ("round", Class::BuiltIn),
        ("self", Class::Variable),
        ("set", Class::BuiltIn),
        ("setattr", Class::BuiltIn),
        ("slice", Class::BuiltIn),
        ("sorted", Class::BuiltIn),
        ("staticmethod", Class::BuiltIn),
        ("str", Class::BuiltIn),
        ("sum", Class::BuiltIn),
        ("super", Class::BuiltIn),
        ("try", Class::ControlFlow),
        ("tuple", Class::BuiltIn),
        ("type", Class::BuiltIn),
        ("unichr", Class::BuiltIn),
        ("unicode", Class::BuiltIn),
        ("vars", Class::BuiltIn),
        ("while", Class::ControlFlow),
        ("with", Class::ControlFlow),
        ("xrange", Class::BuiltIn),
        ("yield", Class::ControlFlow),
        ("zip", Class::BuiltIn),
    ],
    line_comment: &["#"],
    block_comment: None,
    quotes: &[
        ('"', Class::Str),
        ('\'', Class::Str),
    ],
    // `%20` inside a string is an `sc` here as `%s` is in C — the same
    // conversion rule, in a language that also spells modulo `%`.
    quirks: CONVERSIONS | DOCSTRINGS | DECORATORS | PLACEHOLDERS,
    operators: PYTHON_OPERATORS,
    string_prefixes: "rbfuRBFU",
    digit_separator: Some('_'),
    escape: Class::Char,
};

static SYNTAXES: &[&Syntax] = &[&C, &PYTHON];

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
    /// Inside a `"""`/`'''` run: the quote character and the class the
    /// run took where it opened, which is a `co` for a docstring.
    open_string: Option<(char, Class)>,
    /// How many `(`, `[` or `{` are still open. A string that opens a
    /// line is a docstring **only at depth zero** — inside a bracket run
    /// it is an ordinary string, which is what a `__all__ = [` list is
    /// full of. Measured three ways round.
    brackets: usize,
}

/// One line, as a run of `(class, text)` pieces with adjacent pieces of
/// the same class already merged — pandoc emits one span per run.
pub(crate) fn line(text: &str, name: &str, state: &mut State) -> Vec<(Class, String)> {
    let Some(syntax) = syntax(name) else {
        return vec![(Class::Normal, text.to_owned())];
    };
    let mut out: Vec<(Class, String)> = Vec::new();
    let mut at = 0;
    if let Some((quote, class)) = state.open_string {
        let delimiter: String = std::iter::repeat_n(quote, 3).collect();
        // An **empty** line inside the run is empty: pandoc writes no
        // span at all for it, not an empty one.
        if text.is_empty() {
            return Vec::new();
        }
        let end = text.find(&delimiter);
        let body = end.map_or(text, |end| &text[..end]);
        placeholders(body, class, syntax, &mut out);
        match end {
            None => return out,
            Some(end) => {
                push(&mut out, class, &text[end..end + delimiter.len()]);
                at = end + delimiter.len();
                state.open_string = None;
            }
        }
    } else if state.in_block_comment {
        let (_, close) = syntax.block_comment.expect("only set inside one");
        match text.find(close) {
            None => return vec![(Class::Comment, text.to_owned())],
            Some(end) => {
                push(&mut out, Class::Comment, &text[..end + close.len()]);
                at = end + close.len();
                state.in_block_comment = false;
            }
        }
    } else if syntax.has(PREPROCESSOR) {
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
        // `@decorator`, but only where it opens the line.
        if syntax.has(DECORATORS)
            && rest.starts_with('@')
            && out.iter().all(|(_, run)| run.trim().is_empty())
        {
            let end = rest[1..]
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .map_or(rest.len(), |index| index + 1);
            push(out, Class::Attribute, &rest[..end]);
            at += end;
            continue;
        }
        if let Some(mut opener) = opening(rest, syntax) {
            // A string that opens the line is a docstring, and pandoc
            // colours it as a comment.
            // An **f-string is never a docstring**, measured: a bare
            // `f"…"` on its own line stays a `ss`, where a bare `r"…"`
            // becomes a `co`.
            if syntax.has(DOCSTRINGS)
                && opener.class == Class::Str
                && state.brackets == 0
                && out.iter().all(|(_, run)| run.trim().is_empty())
            {
                opener.class = Class::Comment;
            }
            at = quoted(text, at, opener, syntax, state, out);
            continue;
        }
        let byte = bytes[at];
        if byte.is_ascii_digit() {
            at = number(text, at, syntax.digit_separator, out);
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
        if syntax.operators.contains(char::from(byte)) {
            let run = rest.find(|c: char| !syntax.operators.contains(c)).unwrap_or(rest.len());
            push(out, Class::Operator, &rest[..run]);
            at += run;
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        match &rest[..width] {
            "(" | "[" | "{" => state.brackets += 1,
            ")" | "]" | "}" => state.brackets = state.brackets.saturating_sub(1),
            _ => {}
        }
        push(out, Class::Normal, &rest[..width]);
        at += width;
    }
}

/// A quoted run: the quotes and the text are the run's own class, an
/// escape sequence is `sc`, and so is a `printf` conversion.
/// Where a quoted run starts: how many prefix letters to step over, the
/// quote character, and the class the run takes.
#[derive(Clone, Copy)]
struct Opener {
    skip: usize,
    quote: char,
    class: Class,
}

/// The quoted run that opens `rest`, if one does. A prefix letter belongs
/// to the string it introduces (`r"…"`), and `f` makes the run a `ss`
/// whose braces are `sc`.
fn opening(rest: &str, syntax: &Syntax) -> Option<Opener> {
    let first = rest.chars().next()?;
    if let Some((_, class)) = syntax.quotes.iter().find(|(q, _)| *q == first) {
        return Some(Opener { skip: 0, quote: first, class: *class });
    }
    if !syntax.string_prefixes.contains(first) {
        return None;
    }
    let quote = rest[first.len_utf8()..].chars().next()?;
    syntax.quotes.iter().find(|(q, _)| *q == quote)?;
    let class = if first.eq_ignore_ascii_case(&'f') {
        Class::SpecialString
    } else {
        Class::Str
    };
    Some(Opener { skip: first.len_utf8(), quote, class })
}

fn quoted(
    text: &str,
    from: usize,
    opener: Opener,
    syntax: &Syntax,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
) -> usize {
    let Opener { skip, quote, class } = opener;
    // `"""` and `'''` close on the same run of three, not on one.
    let triple: String = std::iter::repeat_n(quote, 3).collect();
    let delimiter = if text[from + skip..].starts_with(&triple) {
        triple.as_str()
    } else {
        &triple[..quote.len_utf8()]
    };
    push(out, class, &text[from..from + skip + delimiter.len()]);
    let mut at = from + skip + delimiter.len();
    while at < text.len() {
        let rest = &text[at..];
        if syntax.has(CONVERSIONS) && class == Class::Str
            && let Some(len) = specifier(rest)
        {
            push(out, Class::SpecialChar, &rest[..len]);
            at += len;
            continue;
        }
        // A backslash that ends the line is a continuation, and pandoc
        // classes it `op` rather than as an escape.
        if rest == "\\" {
            push(out, Class::Operator, rest);
            return at + 1;
        }
        if rest.starts_with('\\') {
            let mut end = 0;
            while rest[end..].starts_with('\\') {
                end = (end + 2).min(rest.len());
            }
            push(out, syntax.escape, &rest[..end]);
            at += end;
            continue;
        }
        // `{…}` in an ordinary string is one piece; in an f-string the
        // braces are the pieces and the code between them is not.
        if syntax.has(PLACEHOLDERS)
            && class != Class::SpecialString
            && rest.starts_with('{')
            && let Some(end) = rest.find('}')
        {
            push(out, Class::SpecialChar, &rest[..=end]);
            at += end + 1;
            continue;
        }
        // An f-string's braces are the `sc` its escapes would be, and
        // what lies between them is ordinary code.
        if class == Class::SpecialString && rest.starts_with('{') {
            push(out, Class::SpecialChar, &rest[..1]);
            at += 1;
            continue;
        }
        if class == Class::SpecialString && rest.starts_with('}') {
            push(out, Class::SpecialChar, &rest[..1]);
            at += 1;
            continue;
        }

        if class == Class::SpecialString
            && out.last().is_some_and(|(kind, run)| *kind == Class::SpecialChar && run.ends_with('{'))
        {
            at += interior(rest, syntax, state, out);
            continue;
        }
        if rest.starts_with(delimiter) {
            push(out, class, &rest[..delimiter.len()]);
            return at + delimiter.len();
        }
        let stop = rest
            .char_indices()
            .find(|(index, c)| {
                *index > 0
                    && (*c == quote
                        || *c == '\\'
                        || (syntax.has(CONVERSIONS) && *c == '%')
                        || (syntax.has(PLACEHOLDERS) && (*c == '{' || *c == '}')))
            })
            .map_or(rest.len(), |(index, _)| index);
        push(out, class, &rest[..stop]);
        at += stop;
    }
    // The line ended inside the run: a triple-quoted one continues on the
    // next, a single-quoted one does not.
    if delimiter.len() > quote.len_utf8() {
        state.open_string = Some((quote, class));
    }
    at
}

/// What lies between an f-string's braces is **code**, and is tokenized
/// as such: `f"{len(x) - 1}"` has a `bu` and an `op` in it. Returns how
/// much of `rest` it consumed.
///
/// Two rules that only hold here. An attribute dot is an `sc`, but only
/// at the **top level** of the braces — `{a.b}` has one and `{len(a.b)}`
/// does not, measured both ways; nowhere else in python is a dot
/// anything but plain text. And `{x:.2f}` — the conversion after the
/// colon belongs to the brace that closes it, where a `:` outside the
/// braces is ordinary string text.
fn interior(rest: &str, syntax: &Syntax, state: &mut State, out: &mut Vec<(Class, String)>) -> usize {
    let end = rest.find('}').unwrap_or(rest.len());
    let spec = rest[..end].find(':');
    let code = spec.unwrap_or(end);
    let mut depth = 0usize;
    let mut piece = 0;
    for (index, c) in rest[..code].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => {
                scan(&rest[piece..index], 0, syntax, state, out);
                push(out, Class::SpecialChar, ".");
                piece = index + 1;
            }
            _ => {}
        }
    }
    scan(&rest[piece..code], 0, syntax, state, out);
    if spec.is_some() {
        push(out, Class::SpecialChar, &rest[code..=end]);
        return end + 1;
    }
    code
}

/// A stretch of string content, with `{…}` placeholders taken out of it.
/// Used where a `"""` run continues onto the next line and there is no
/// opening quote to scan past.
fn placeholders(text: &str, class: Class, syntax: &Syntax, out: &mut Vec<(Class, String)>) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        // A backslash that ends the line is a continuation, not an escape.
        if rest == "\\" {
            push(out, Class::Operator, rest);
            return;
        }
        if syntax.has(CONVERSIONS)
            && let Some(len) = specifier(rest)
        {
            push(out, Class::SpecialChar, &rest[..len]);
            at += len;
            continue;
        }
        if syntax.has(PLACEHOLDERS)
            && rest.starts_with('{')
            && let Some(end) = rest.find('}')
        {
            push(out, Class::SpecialChar, &rest[..=end]);
            at += end + 1;
            continue;
        }
        let stop = rest[1..]
            .find(['{', '%', '\\'])
            .map_or(rest.len(), |index| index + 1);
        push(out, class, &rest[..stop]);
        at += stop;
    }
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
fn number(
    text: &str,
    from: usize,
    separator: Option<char>,
    out: &mut Vec<(Class, String)>,
) -> usize {
    let rest = &text[from..];
    let based = ["0x", "0X", "0b", "0B", "0o", "0O"]
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
            let digit = |c: char| c.is_ascii_digit() || Some(c) == separator;
            let digits = rest.find(|c: char| !digit(c)).unwrap_or(rest.len());
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

    /// Python's rules, which are a different order of hair from C's.
    #[test]
    fn python_is_tokenized_the_way_skylighting_tokenizes_it() {
        let text = |pieces: &[(Class, &str)]| {
            pieces.iter().map(|(c, t)| (*c, (*t).to_owned())).collect::<Vec<_>>()
        };
        // A string that opens a line is a docstring — a comment.
        assert_eq!(classes("\"doc\"", "python"), text(&[(Class::Comment, "\"doc\"")]));
        // …but not inside a bracket run, which is what a `__all__` list
        // is full of.
        let mut state = State::default();
        line("__all__ = [", "python", &mut state);
        assert_eq!(
            line("    \"a\",", "python", &mut state),
            text(&[(Class::Normal, "    "), (Class::Str, "\"a\""), (Class::Normal, ",")])
        );
        // `{…}` in an ordinary string is one piece; in an f-string the
        // braces are the pieces and what lies between them is code.
        assert_eq!(
            classes("x = \"a {b} c\"", "python"),
            text(&[
                (Class::Normal, "x "),
                (Class::Operator, "="),
                (Class::Normal, " "),
                (Class::Str, "\"a "),
                (Class::SpecialChar, "{b}"),
                (Class::Str, " c\""),
            ])
        );
        assert_eq!(
            classes("f\"{len(a)}\"", "python"),
            text(&[
                (Class::SpecialString, "f\""),
                (Class::SpecialChar, "{"),
                (Class::BuiltIn, "len"),
                (Class::Normal, "(a)"),
                (Class::SpecialChar, "}"),
                (Class::SpecialString, "\""),
            ])
        );
        // The attribute dot inside those braces is an `sc`, but only at
        // their top level.
        assert_eq!(classes("f\"{a.b}\"", "python")[3], (Class::SpecialChar, ".".to_owned()));
        assert_eq!(classes("f\"{len(a.b)}\"", "python")[3], (Class::Normal, "(a.b)".to_owned()));
        // Python's operator set is not C's: brackets, commas, dots and
        // colons are plain text here.
        assert_eq!(
            classes("f(a, b)", "python"),
            text(&[(Class::Normal, "f(a, b)")])
        );
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
