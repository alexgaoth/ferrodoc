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
    Extension,
    Other,
    /// Ruby's single-quoted string, which interpolates nothing.
    VerbatimString,
    /// A capitalised name — Ruby's constants.
    Constant,
    /// A word pandoc marks inside a comment: `TODO`, `FIXME`, `###`.
    Alert,
    /// What `%x( … )` — a shell command — comes back as.
    Information,
    /// A backslash that begins no escape python knows.
    Error,
    /// A python docstring. Rendered `co` like a comment, and kept apart
    /// from one because **a docstring carries no alert words**: `TODO`
    /// in a `#` comment is an `al` and `TODO` in a `""" … """` is not.
    Documentation,
    /// **A Ruby symbol.** Skylighting files `:name` under the class it
    /// uses for warnings; probed, not guessed.
    Warning,
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
            // A docstring renders as a comment and is not one: it
            // carries no alert words, which is why it has its own variant.
            Class::Comment | Class::Documentation => "co",
            Class::Operator => "op",
            Class::Preprocessor => "pp",
            Class::Import => "im",
            Class::Variable => "va",
            Class::Attribute => "at",
            Class::SpecialString => "ss",
            Class::Function => "fu",
            Class::Extension => "ex",
            Class::Other => "ot",
            Class::VerbatimString => "vs",
            Class::Constant => "cn",
            Class::Alert => "al",
            Class::Information => "in",
            Class::Error => "er",
            Class::Warning => "wa",
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
    /// The letters that end a `%…` conversion, which differ by language.
    conversions: &'static str,
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
/// A backslash begins an escape only where python says it does, and
/// otherwise is an `er`: `"\n"` is a `ch` and `"\d"` is an error.
const STRICT_ESCAPES: u8 = 1 << 5;
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
    // Probed a-z and A-Z one at a time, in `x = "%c"`. C takes `%a` and
    // `%b`, which python does not; python takes `%r`, which C does not.
    conversions: "abcdefginopsuxAEFGX%",
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
        // The dunders were probed the same way, 96 of them: 68 are
        // `fu` wherever they stand — `x.__init__` too — while
        // `__name__` and `__file__` are `va`, and 29 others,
        // `__dict__` and `__doc__` among them, carry no class at all.
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
        ("__abs__", Class::Function),
        ("__add__", Class::Function),
        ("__aenter__", Class::Function),
        ("__aexit__", Class::Function),
        ("__aiter__", Class::Function),
        ("__all__", Class::Variable),
        ("__and__", Class::Function),
        ("__anext__", Class::Function),
        ("__await__", Class::Function),
        ("__bool__", Class::Function),
        ("__bytes__", Class::Function),
        ("__call__", Class::Function),
        ("__ceil__", Class::Function),
        ("__class__", Class::Variable),
        ("__class_getitem__", Class::Function),
        ("__complex__", Class::Function),
        ("__contains__", Class::Function),
        ("__debug__", Class::Variable),
        ("__del__", Class::Function),
        ("__delattr__", Class::Function),
        ("__delete__", Class::Function),
        ("__delitem__", Class::Function),
        ("__dir__", Class::Function),
        ("__enter__", Class::Function),
        ("__eq__", Class::Function),
        ("__exit__", Class::Function),
        ("__file__", Class::Variable),
        ("__float__", Class::Function),
        ("__floor__", Class::Function),
        ("__floordiv__", Class::Function),
        ("__format__", Class::Function),
        ("__ge__", Class::Function),
        ("__get__", Class::Function),
        ("__getattr__", Class::Function),
        ("__getitem__", Class::Function),
        ("__gt__", Class::Function),
        ("__hash__", Class::Function),
        ("__import__", Class::BuiltIn),
        ("__index__", Class::Function),
        ("__init__", Class::Function),
        ("__init_subclass__", Class::Function),
        ("__instancecheck__", Class::Function),
        ("__int__", Class::Function),
        ("__invert__", Class::Function),
        ("__iter__", Class::Function),
        ("__le__", Class::Function),
        ("__len__", Class::Function),
        ("__length_hint__", Class::Function),
        ("__lshift__", Class::Function),
        ("__lt__", Class::Function),
        ("__match_args__", Class::Function),
        ("__missing__", Class::Function),
        ("__mod__", Class::Function),
        ("__mul__", Class::Function),
        ("__name__", Class::Variable),
        ("__ne__", Class::Function),
        ("__neg__", Class::Function),
        ("__new__", Class::Function),
        ("__next__", Class::Function),
        ("__or__", Class::Function),
        ("__pow__", Class::Function),
        ("__qualname__", Class::Variable),
        ("__repr__", Class::Function),
        ("__round__", Class::Function),
        ("__rshift__", Class::Function),
        ("__set__", Class::Function),
        ("__set_name__", Class::Function),
        ("__setattr__", Class::Function),
        ("__setitem__", Class::Function),
        ("__slots__", Class::Variable),
        ("__str__", Class::Function),
        ("__sub__", Class::Function),
        ("__subclasscheck__", Class::Function),
        ("__truediv__", Class::Function),
        ("__trunc__", Class::Function),
        ("__xor__", Class::Function),
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
        ("case", Class::ControlFlow),
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
        ("match", Class::ControlFlow),
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
    quirks: CONVERSIONS | DOCSTRINGS | DECORATORS | PLACEHOLDERS | STRICT_ESCAPES,
    conversions: "cdefgiorsuxEFGX%",
    operators: PYTHON_OPERATORS,
    string_prefixes: "rbfuRBFU",
    digit_separator: Some('_'),
    escape: Class::Char,
};

/// Bash's words. `fu` is a command pandoc knows and `ex` is one it does
/// not, so the table decides only the first of those — everything absent
/// from it is an `ex`, which is why a missing name is a visible
/// divergence on a real script rather than a silent one.
static BASH: Syntax = Syntax {
    conversions: "",
    canonical: "bash",
    names: &["bash", "sh", "shell", "zsh", "ksh"],
    keywords: &[
        (".", Class::BuiltIn),
        (":", Class::BuiltIn),
        ("aconnect", Class::Function),
        ("alias", Class::BuiltIn),
        ("aplay", Class::Function),
        ("apropos", Class::Function),
        ("ar", Class::Function),
        ("arch", Class::Function),
        ("arecord", Class::Function),
        ("as", Class::Function),
        ("awk", Class::Function),
        ("b2sum", Class::Function),
        ("base32", Class::Function),
        ("base64", Class::Function),
        ("basename", Class::Function),
        ("bash", Class::Function),
        ("bc", Class::Function),
        ("bg", Class::BuiltIn),
        ("bind", Class::BuiltIn),
        ("bison", Class::Function),
        ("break", Class::ControlFlow),
        ("builtin", Class::BuiltIn),
        ("bunzip2", Class::Function),
        ("bzcat", Class::Function),
        ("bzcmp", Class::Function),
        ("bzdiff", Class::Function),
        ("bzegrep", Class::Function),
        ("bzfgrep", Class::Function),
        ("bzgrep", Class::Function),
        ("bzip2", Class::Function),
        ("bzip2recover", Class::Function),
        ("bzless", Class::Function),
        ("bzmore", Class::Function),
        ("cal", Class::Function),
        ("caller", Class::BuiltIn),
        ("case", Class::ControlFlow),
        ("cat", Class::Function),
        ("cc", Class::Function),
        ("cd", Class::BuiltIn),
        ("cd-read", Class::Function),
        ("cdrecord", Class::Function),
        ("chattr", Class::Function),
        ("chcon", Class::Function),
        ("chfn", Class::Function),
        ("chgrp", Class::Function),
        ("chmod", Class::Function),
        ("chown", Class::Function),
        ("chroot", Class::Function),
        ("chsh", Class::Function),
        ("chvt", Class::Function),
        ("cksum", Class::Function),
        ("clear", Class::Function),
        ("cmp", Class::Function),
        ("col", Class::Function),
        ("comm", Class::Function),
        ("command", Class::BuiltIn),
        ("compgen", Class::BuiltIn),
        ("complete", Class::BuiltIn),
        ("continue", Class::ControlFlow),
        ("coproc", Class::BuiltIn),
        ("cp", Class::Function),
        ("cpio", Class::Function),
        ("cpp", Class::Function),
        ("crontab", Class::Function),
        ("csplit", Class::Function),
        ("cut", Class::Function),
        ("date", Class::Function),
        ("dc", Class::Function),
        ("dd", Class::Function),
        ("deallocvt", Class::Function),
        ("declare", Class::BuiltIn),
        ("df", Class::Function),
        ("diff", Class::Function),
        ("diff3", Class::Function),
        ("dir", Class::Function),
        ("dircolors", Class::Function),
        ("dirname", Class::Function),
        ("dirs", Class::BuiltIn),
        ("disown", Class::BuiltIn),
        ("dmesg", Class::Function),
        ("dnsdomainname", Class::Function),
        ("do", Class::ControlFlow),
        ("domainname", Class::Function),
        ("done", Class::ControlFlow),
        ("du", Class::Function),
        ("dumpkeys", Class::Function),
        ("echo", Class::BuiltIn),
        ("ed", Class::Function),
        ("egrep", Class::Function),
        ("elif", Class::ControlFlow),
        ("else", Class::ControlFlow),
        ("enable", Class::BuiltIn),
        ("env", Class::Function),
        ("esac", Class::ControlFlow),
        ("eval", Class::BuiltIn),
        ("exec", Class::BuiltIn),
        ("exit", Class::BuiltIn),
        ("expand", Class::Function),
        ("export", Class::BuiltIn),
        ("expr", Class::Function),
        ("false", Class::Function),
        ("fc", Class::BuiltIn),
        ("fg", Class::BuiltIn),
        ("fgconsole", Class::Function),
        ("fgrep", Class::Function),
        ("fi", Class::ControlFlow),
        ("file", Class::Function),
        ("find", Class::Function),
        ("flex", Class::Function),
        ("fmt", Class::Function),
        ("fold", Class::Function),
        ("for", Class::ControlFlow),
        ("free", Class::Function),
        ("ftp", Class::Function),
        ("function", Class::Keyword),
        ("funzip", Class::Function),
        ("fuser", Class::Function),
        ("gawk", Class::Function),
        ("gcc", Class::Function),
        ("gdb", Class::Function),
        ("getent", Class::Function),
        ("getkeycodes", Class::Function),
        ("getopt", Class::Function),
        ("getopts", Class::BuiltIn),
        ("gettext", Class::Function),
        ("git", Class::Function),
        ("gmake", Class::Function),
        ("grep", Class::Function),
        ("groff", Class::Function),
        ("groups", Class::Function),
        ("gs", Class::Function),
        ("gunzip", Class::Function),
        ("gzexe", Class::Function),
        ("gzip", Class::Function),
        ("hash", Class::BuiltIn),
        ("head", Class::Function),
        ("help", Class::BuiltIn),
        ("hexdump", Class::Function),
        ("history", Class::BuiltIn),
        ("hostid", Class::Function),
        ("hostname", Class::Function),
        ("iconv", Class::Function),
        ("id", Class::Function),
        ("if", Class::ControlFlow),
        ("install", Class::Function),
        ("jobs", Class::BuiltIn),
        ("join", Class::Function),
        ("kbd_mode", Class::Function),
        ("kbdrate", Class::Function),
        ("kill", Class::BuiltIn),
        ("killall", Class::Function),
        ("last", Class::Function),
        ("lastb", Class::Function),
        ("ld", Class::Function),
        ("ldd", Class::Function),
        ("less", Class::Function),
        ("let", Class::BuiltIn),
        ("lex", Class::Function),
        ("link", Class::Function),
        ("ln", Class::Function),
        ("loadkeys", Class::Function),
        ("loadunimap", Class::Function),
        ("local", Class::BuiltIn),
        ("locate", Class::Function),
        ("login", Class::Function),
        ("logname", Class::Function),
        ("logout", Class::BuiltIn),
        ("lp", Class::Function),
        ("lpr", Class::Function),
        ("ls", Class::Function),
        ("lsattr", Class::Function),
        ("lsmod", Class::Function),
        ("m4", Class::Function),
        ("make", Class::Function),
        ("man", Class::Function),
        ("mapscrn", Class::Function),
        ("md5sum", Class::Function),
        ("mesg", Class::Function),
        ("mkdir", Class::Function),
        ("mkfifo", Class::Function),
        ("mknod", Class::Function),
        ("mktemp", Class::Function),
        ("more", Class::Function),
        ("mount", Class::Function),
        ("msgfmt", Class::Function),
        ("mv", Class::Function),
        ("namei", Class::Function),
        ("nano", Class::Function),
        ("netstat", Class::Function),
        ("nice", Class::Function),
        ("nisdomainname", Class::Function),
        ("nl", Class::Function),
        ("nm", Class::Function),
        ("nohup", Class::Function),
        ("nproc", Class::Function),
        ("nroff", Class::Function),
        ("numfmt", Class::Function),
        ("od", Class::Function),
        ("openvt", Class::Function),
        ("passwd", Class::Function),
        ("paste", Class::Function),
        ("patch", Class::Function),
        ("pathchk", Class::Function),
        ("perl", Class::Function),
        ("pidof", Class::Function),
        ("ping", Class::Function),
        ("pinky", Class::Function),
        ("popd", Class::BuiltIn),
        ("pr", Class::Function),
        ("printenv", Class::Function),
        ("printf", Class::BuiltIn),
        ("ps", Class::Function),
        ("ps2ascii", Class::Function),
        ("ps2epsi", Class::Function),
        ("ps2pdf", Class::Function),
        ("ps2ps", Class::Function),
        ("pstree", Class::Function),
        ("ptx", Class::Function),
        ("pushd", Class::BuiltIn),
        ("pwd", Class::BuiltIn),
        ("read", Class::BuiltIn),
        ("readlink", Class::Function),
        ("readonly", Class::BuiltIn),
        ("realpath", Class::Function),
        ("red", Class::Function),
        ("resizecons", Class::Function),
        ("return", Class::ControlFlow),
        ("rev", Class::Function),
        ("rm", Class::Function),
        ("rmdir", Class::Function),
        ("rsync", Class::Function),
        ("run-parts", Class::Function),
        ("runcon", Class::Function),
        ("scp", Class::Function),
        ("sed", Class::Function),
        ("select", Class::ControlFlow),
        ("seq", Class::Function),
        ("set", Class::BuiltIn),
        ("setfont", Class::Function),
        ("setkeycodes", Class::Function),
        ("setleds", Class::Function),
        ("setmetamode", Class::Function),
        ("setterm", Class::Function),
        ("sh", Class::Function),
        ("sha1sum", Class::Function),
        ("sha224sum", Class::Function),
        ("sha256sum", Class::Function),
        ("sha384sum", Class::Function),
        ("sha512sum", Class::Function),
        ("shift", Class::BuiltIn),
        ("shopt", Class::BuiltIn),
        ("showkey", Class::Function),
        ("shred", Class::Function),
        ("shuf", Class::Function),
        ("size", Class::Function),
        ("skill", Class::Function),
        ("sleep", Class::Function),
        ("snice", Class::Function),
        ("sort", Class::Function),
        ("source", Class::BuiltIn),
        ("split", Class::Function),
        ("ssh", Class::Function),
        ("ssh-add", Class::Function),
        ("ssh-agent", Class::Function),
        ("ssh-keygen", Class::Function),
        ("ssh-keyscan", Class::Function),
        ("stat", Class::Function),
        ("stdbuf", Class::Function),
        ("strings", Class::Function),
        ("strip", Class::Function),
        ("stty", Class::Function),
        ("su", Class::Function),
        ("sudo", Class::Function),
        ("sum", Class::Function),
        ("suspend", Class::BuiltIn),
        ("sync", Class::Function),
        ("tac", Class::Function),
        ("tail", Class::Function),
        ("tar", Class::Function),
        ("tee", Class::Function),
        ("test", Class::BuiltIn),
        ("then", Class::ControlFlow),
        ("time", Class::BuiltIn),
        ("timeout", Class::Function),
        ("times", Class::BuiltIn),
        ("touch", Class::Function),
        ("tput", Class::Function),
        ("tr", Class::Function),
        ("trap", Class::BuiltIn),
        ("troff", Class::Function),
        ("true", Class::Function),
        ("truncate", Class::Function),
        ("tsort", Class::Function),
        ("tty", Class::Function),
        ("type", Class::BuiltIn),
        ("typeset", Class::BuiltIn),
        ("ulimit", Class::BuiltIn),
        ("umask", Class::BuiltIn),
        ("umount", Class::Function),
        ("unalias", Class::BuiltIn),
        ("uname", Class::Function),
        ("unexpand", Class::Function),
        ("unicode_start", Class::Function),
        ("unicode_stop", Class::Function),
        ("uniq", Class::Function),
        ("unlink", Class::Function),
        ("unset", Class::BuiltIn),
        ("until", Class::ControlFlow),
        ("unxz", Class::Function),
        ("unzip", Class::Function),
        ("updatedb", Class::Function),
        ("uptime", Class::Function),
        ("users", Class::Function),
        ("utmpdump", Class::Function),
        ("uuidgen", Class::Function),
        ("valgrind", Class::Function),
        ("vdir", Class::Function),
        ("vi", Class::Function),
        ("vmstat", Class::Function),
        ("w", Class::Function),
        ("wait", Class::BuiltIn),
        ("wall", Class::Function),
        ("wc", Class::Function),
        ("wget", Class::Function),
        ("whatis", Class::Function),
        ("whereis", Class::Function),
        ("which", Class::Function),
        ("while", Class::ControlFlow),
        ("who", Class::Function),
        ("whoami", Class::Function),
        ("write", Class::Function),
        ("xargs", Class::Function),
        ("xdg-open", Class::Function),
        ("xhost", Class::Function),
        ("xmodmap", Class::Function),
        ("xz", Class::Function),
        ("xzcat", Class::Function),
        ("yes", Class::Function),
        ("ypdomainname", Class::Function),
        ("zcat", Class::Function),
        ("zcmp", Class::Function),
        ("zdiff", Class::Function),
        ("zegrep", Class::Function),
        ("zfgrep", Class::Function),
        ("zforce", Class::Function),
        ("zgrep", Class::Function),
        ("zip", Class::Function),
        ("zless", Class::Function),
        ("zmore", Class::Function),
        ("znew", Class::Function),
        ("zsh", Class::Function),
        ("zsoelim", Class::Function),
    ],
    line_comment: &["#"],
    block_comment: None,
    quotes: &[('"', Class::Str), ('\'', Class::Str)],
    quirks: 0,
    operators: "",
    string_prefixes: "",
    digit_separator: None,
    escape: Class::SpecialChar,
};


/// Ruby, whose classes are the least guessable of the four.
///
/// **`true`, `false`, `nil`, `self` and `super` are `dv`** — the class
/// skylighting gives a decimal number — and a **symbol is `wa`**, the
/// class it gives a warning. Neither is a slip: `ROADMAP.md` recorded
/// both from a probe before any of this was written, and every word here
/// was read back from the pinned binary one at a time, the way bash's
/// 204 were.
static RUBY: Syntax = Syntax {
    conversions: "",
    canonical: "ruby",
    names: &["ruby", "rb"],
    // Probed over **Ruby's own vocabulary**, the way python's was:
    // `Kernel.private_instance_methods` plus `Kernel.methods` from the
    // 3.4.3 on this machine, 95 names, each read back one at a time.
    // 50 are `fu`, `private`/`public`/`protected` are `at`, and
    // `caller` sits with `true` and `nil` as a `dv`.
    keywords: &[
        ("BEGIN", Class::ControlFlow),
        ("END", Class::ControlFlow),
        ("abort", Class::Function),
        ("alias", Class::Keyword),
        ("and", Class::ControlFlow),
        ("at_exit", Class::Function),
        ("attr_accessor", Class::Other),
        ("attr_reader", Class::Other),
        ("attr_writer", Class::Other),
        ("autoload", Class::Function),
        ("autoload?", Class::Function),
        ("begin", Class::ControlFlow),
        ("binding", Class::Function),
        ("block_given?", Class::Function),
        ("break", Class::ControlFlow),
        ("caller", Class::DecVal),
        ("case", Class::ControlFlow),
        ("catch", Class::Function),
        ("class", Class::ControlFlow),
        ("def", Class::ControlFlow),
        ("defined?", Class::ControlFlow),
        ("do", Class::ControlFlow),
        ("else", Class::ControlFlow),
        ("elsif", Class::ControlFlow),
        ("end", Class::ControlFlow),
        ("ensure", Class::ControlFlow),
        ("eval", Class::Function),
        ("exec", Class::Function),
        ("exit", Class::Function),
        ("exit!", Class::Function),
        ("extend", Class::Function),
        ("fail", Class::Function),
        ("false", Class::DecVal),
        ("for", Class::ControlFlow),
        ("fork", Class::Function),
        ("format", Class::Function),
        ("gets", Class::Function),
        ("global_variables", Class::Function),
        ("if", Class::ControlFlow),
        ("in", Class::ControlFlow),
        ("include", Class::Function),
        ("iterator?", Class::Function),
        ("lambda", Class::Function),
        ("load", Class::Function),
        ("local_variables", Class::Function),
        ("loop", Class::Function),
        ("module", Class::ControlFlow),
        ("next", Class::ControlFlow),
        ("nil", Class::DecVal),
        ("not", Class::ControlFlow),
        ("open", Class::Function),
        ("or", Class::ControlFlow),
        ("p", Class::Function),
        ("prepend", Class::Function),
        ("print", Class::Function),
        ("printf", Class::Function),
        ("private", Class::Attribute),
        ("proc", Class::Function),
        ("protected", Class::Attribute),
        ("public", Class::Attribute),
        ("putc", Class::Function),
        ("puts", Class::Function),
        ("raise", Class::Function),
        ("rand", Class::Function),
        ("readline", Class::Function),
        ("readlines", Class::Function),
        ("redo", Class::ControlFlow),
        ("require", Class::Function),
        ("require_relative", Class::Function),
        ("rescue", Class::ControlFlow),
        ("retry", Class::ControlFlow),
        ("return", Class::ControlFlow),
        ("select", Class::Function),
        ("self", Class::DecVal),
        ("set_trace_func", Class::Function),
        ("sleep", Class::Function),
        ("sprintf", Class::Function),
        ("srand", Class::Function),
        ("super", Class::DecVal),
        ("syscall", Class::Function),
        ("system", Class::Function),
        ("test", Class::Function),
        ("then", Class::ControlFlow),
        ("throw", Class::Function),
        ("trace_var", Class::Function),
        ("trap", Class::Function),
        ("true", Class::DecVal),
        ("undef", Class::Keyword),
        ("unless", Class::ControlFlow),
        ("until", Class::ControlFlow),
        ("untrace_var", Class::Function),
        ("warn", Class::Function),
        ("when", Class::ControlFlow),
        ("while", Class::ControlFlow),
        ("yield", Class::ControlFlow),
    ],
    line_comment: &["#"],
    block_comment: None,
    quotes: &[('"', Class::Str), ('\'', Class::VerbatimString)],
    quirks: 0,
    // Probed one character at a time, each in its own document — a
    // single document with one line per character puts an unterminated
    // `"` in the middle of it and every line after that comes back `st`.
    // `(`, `)`, `,` and `;` are **plain** in Ruby where C makes them
    // operators; `[` and `]` are `kw`; `.` is an `at`.
    operators: "!%&*+-/:<=>?^{|}~",
    string_prefixes: "",
    digit_separator: Some('_'),
    escape: Class::SpecialChar,
};

static SYNTAXES: &[&Syntax] = &[&C, &PYTHON, &BASH, &RUBY];

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
/// What a line can leave open for the next one to finish.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Carried {
    Nothing,
    /// A `/* … */` that has not closed.
    BlockComment,
    /// A preprocessor directive ended with `\`, so the next line is its.
    Directive,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct State {
    /// What the line before left open, if anything.
    carried: Carried,
    /// Inside a `"""`/`'''` run: the quote character and the class the
    /// run took where it opened, which is a `co` for a docstring.
    /// The quote, the class the run took, and whether its body is read
    /// as a regular expression.
    open_string: Option<(char, Class, bool)>,
    /// Bash only: where in a command the scanner stands. Carried
    /// between lines because a line ending in `\\`, `|` or `&&`
    /// continues one.
    position: Position,
    /// Bash only: inside `[ … ]`, where `-x` is an `ot` rather than the
    /// `at` it would be after a command.
    in_test: bool,
    /// Bash only: between a `case … in` and its `esac`.
    in_case: bool,
    /// Bash only: the delimiter of an open here-document, and whether it
    /// expands variables — which an unquoted delimiter does.
    heredoc: Option<(String, bool)>,
    /// Bash only: how many `$( … )` substitutions and how many plain
    /// `( … )` groups are still open, which together tell a `)` on a
    /// later line whose it is.
    subst: usize,
    parens: usize,
    /// How many `(`, `[` or `{` are still open. A string that opens a
    /// line is a docstring **only at depth zero** — inside a bracket run
    /// it is an ordinary string, which is what a `__all__ = [` list is
    /// full of. Measured three ways round.
    brackets: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            carried: Carried::Nothing,
            open_string: None,
            // A code block opens at the start of a command.
            position: Position::Command,
            in_test: false,
            in_case: false,
            heredoc: None,
            subst: 0,
            parens: 0,
            brackets: 0,
        }
    }
}

/// One line, as a run of `(class, text)` pieces with adjacent pieces of
/// the same class already merged — pandoc emits one span per run.
pub(crate) fn line(text: &str, name: &str, state: &mut State) -> Vec<(Class, String)> {
    alerted(uncommented(text, name, state))
}

/// **Words pandoc marks inside a comment, whatever the language is.**
/// Probed against C, python, bash and ruby, which all four agree; and
/// probed word by word, because the list is not the one it looks like —
/// `XXX`, `REVIEW`, `OPTIMIZE`, `IMPORTANT`, `TIP` and `ERROR` are *not*
/// on it, and `###` is.
const ALERTS: &[&str] = &[
    "###",
    "ALERT",
    "ATTENTION",
    "BUG",
    "CAUTION",
    "DANGER",
    "DEPRECATED",
    "FIXME",
    "HACK",
    "NOTE",
    "NOTICE",
    "SECURITY",
    "TODO",
    "WARNING",
];

/// Whether `#` or `_` or an alphanumeric — the characters that keep an
/// alert word from starting or ending here.
///
/// Counting `#` is what makes `#TODO` plain where `# TODO` is an alert,
/// and `# ####` plain where `# ###` is one. Both were measured; neither
/// would have been guessed.
fn wordish(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '#'
}

/// Split the alert words out of every comment run.
fn alerted(pieces: Vec<(Class, String)>) -> Vec<(Class, String)> {
    if !pieces.iter().any(|(class, text)| {
        *class == Class::Comment && ALERTS.iter().any(|word| text.contains(word))
    }) {
        return pieces;
    }
    let mut out = Vec::with_capacity(pieces.len());
    for (class, text) in pieces {
        if class != Class::Comment {
            out.push((class, text));
            continue;
        }
        let mut at = 0;
        while at < text.len() {
            let before_is_word = text[..at].ends_with(wordish);
            let found = (!before_is_word)
                .then(|| {
                    ALERTS.iter().find(|word| {
                        text[at..].starts_with(**word)
                            && !text[at + word.len()..].starts_with(wordish)
                    })
                })
                .flatten();
            if let Some(word) = found {
                push(&mut out, Class::Alert, word);
                at += word.len();
            } else {
                let width = text[at..].chars().next().map_or(1, char::len_utf8);
                push(&mut out, Class::Comment, &text[at..at + width]);
                at += width;
            }
        }
    }
    out
}

/// One line, before its comments are read for alert words.
fn uncommented(text: &str, name: &str, state: &mut State) -> Vec<(Class, String)> {
    let Some(syntax) = syntax(name) else {
        return vec![(Class::Normal, text.to_owned())];
    };
    if std::ptr::eq(syntax, &raw const RUBY) {
        let mut out = Vec::new();
        ruby(text, state, &mut out);
        return out;
    }
    if std::ptr::eq(syntax, &raw const BASH) {
        let mut out = Vec::new();
        bash(text, state, &mut out);
        return out;
    }
    let mut out: Vec<(Class, String)> = Vec::new();
    let mut at = 0;
    if let Some((quote, class, regexp)) = state.open_string {
        let delimiter: String = std::iter::repeat_n(quote, 3).collect();
        // An **empty** line inside the run is empty: pandoc writes no
        // span at all for it, not an empty one.
        if text.is_empty() {
            return Vec::new();
        }
        // A `r""" … """` continues as a regular expression, not as text:
        // `doctest.py`'s `_EXAMPLE_RE` is twenty lines of one.
        if regexp {
            at = python_regexp(text, 0, &delimiter, class, false, state, &mut out);
            if state.open_string.is_some() {
                return out;
            }
        } else {
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
        }
    } else if state.carried == Carried::BlockComment {
        let (_, close) = syntax.block_comment.expect("only set inside one");
        match text.find(close) {
            // **An empty line inside a block comment carries no span.**
            // Returning one unconditionally put `<span class="co"></span>`
            // on every blank line of every licence header — the single
            // largest source of divergence on real C files.
            None if text.is_empty() => return Vec::new(),
            None => return vec![(Class::Comment, text.to_owned())],
            Some(end) => {
                push(&mut out, Class::Comment, &text[..end + close.len()]);
                at = end + close.len();
                state.carried = Carried::Nothing;
            }
        }
    } else if state.carried == Carried::Directive {
        // **A directive continued with `\` runs on**: the whole of the next
        // line is `pp`, and its own trailing `\` continues it again.
        let end = text.strip_suffix('\\').map_or(text.len(), str::len);
        push(&mut out, Class::Preprocessor, &text[..end]);
        if end < text.len() {
            push(&mut out, Class::Operator, "\\");
        }
        state.carried =
            if end < text.len() { Carried::Directive } else { Carried::Nothing };
        return out;
    } else if syntax.has(PREPROCESSOR) {
        at = directive(text, syntax, &mut out);
        if at > 0 && text.ends_with('\\') {
            state.carried = Carried::Directive;
        }
        if at > 0 {
            scan(text, at, syntax, state, &mut out);
            return preprocessed(out);
        }
    }
    scan(text, at, syntax, state, &mut out);
    out
}

/// **On a directive line nothing is plain.** The spacing before a trailing
/// comment, a macro's parameter names, the body it expands to — all of it
/// comes back `pp`, while the strings, numbers, operators and comments in
/// it keep their own classes. Leaving those runs `Normal` was why real
/// headers diverged on almost every `#define` and `#include`.
fn preprocessed(pieces: Vec<(Class, String)>) -> Vec<(Class, String)> {
    let mut out: Vec<(Class, String)> = Vec::with_capacity(pieces.len());
    for (class, text) in pieces {
        let class = if class == Class::Normal { Class::Preprocessor } else { class };
        match out.last_mut() {
            Some((last, run)) if *last == class => run.push_str(&text),
            _ => out.push((class, text)),
        }
    }
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

/// A two-character operator neither language spells with its operator set.
///
/// **`##` pastes two tokens together and is an `op`**, though the `#` that
/// opens the directive it sits in is a `pp`. **`:=` is an operator; a bare
/// `:` is not** — putting `:` in python's set would colour every dict key
/// and every `def` line.
fn paired_operator(rest: &str, syntax: &Syntax) -> Option<&'static str> {
    if rest.starts_with("##") && syntax.has(PREPROCESSOR) {
        return Some("##");
    }
    if rest.starts_with(":=") && std::ptr::eq(syntax, &raw const PYTHON) {
        return Some(":=");
    }
    None
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
                    state.carried = Carried::BlockComment;
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
            // A raw string can be one too — `doctest.py` opens with
            // `r"""…"""` — and when it is, it is read as prose and not as
            // the regular expression an `r` prefix otherwise makes it.
            if syntax.has(DOCSTRINGS)
                && matches!(opener.class, Class::Str | Class::VerbatimString)
                && state.brackets == 0
                && out.iter().all(|(_, run)| run.trim().is_empty())
            {
                opener.class = Class::Documentation;
                opener.regexp = false;
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
            // **`match` and `case` are soft keywords**: `cf` only when a
            // space and then an operand follow. `match = 1`, `match(x)`
            // and `f(match="a")` are all ordinary names — probed, and the
            // reason a test file in this repo caught the first attempt.
            let class = if (word == "match" || word == "case")
                && std::ptr::eq(syntax, &raw const PYTHON)
                && !rest[word.len()..]
                    .strip_prefix(' ')
                    .is_some_and(|after| after.starts_with(|c: char| c != '=' && c != ' '))
            {
                Class::Normal
            } else {
                class
            };
            push(out, class, &word);
            at += word.len();
            continue;
        }
        if let Some(pair) = paired_operator(rest, syntax) {
            push(out, Class::Operator, pair);
            at += pair.len();
            continue;
        }
        // A backslash alone at the end of a line continues it, and pandoc
        // classes it `op` — the `\` of a multi-line `#define`.
        if rest == "\\" {
            push(out, Class::Operator, rest);
            at += 1;
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
    /// Whether the body is a regular expression, which a lowercase `r`
    /// prefix makes it and a capital `R` does not.
    regexp: bool,
    /// Whether `{ … }` in the body is a placeholder — an `f` prefix, which
    /// can stand beside the `r` and does in `fr"…"`.
    placeholders: bool,
}

/// The body of a python raw string, which pandoc reads as a **regular
/// expression** rather than as text. Every rule below was probed one
/// construct at a time against the pinned binary.
///
/// The shape of it: the body is a `vs`; `\d` and its letter friends are
/// `dv` while `\1` and `\.` are `ch`; a character class is `pp` with its
/// escapes keeping their own classes; `.`, `^` and `$` are `dv`; `|` is
/// `cf`; `+`, `*`, `?` and a **numeric** `{2,3}` are `op` — `{a}` is not;
/// and a group's parentheses take their class from what opens it.
fn python_regexp(
    text: &str,
    from: usize,
    delimiter: &str,
    class: Class,
    placeholders: bool,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
) -> usize {
    // What each open group's `)` will be. `(?: … )` closes with no class
    // at all, which is why this holds an `Option`.
    let mut groups: Vec<Option<Class>> = Vec::new();
    let verbose = delimiter.len() > 1;
    let mut at = from;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with(delimiter) {
            push(out, class, delimiter);
            state.open_string = None;
            return at + delimiter.len();
        }
        // **A triple-quoted raw string is a *verbose* regexp**, where `#`
        // comments to the end of the line — and a single-quoted one is
        // not: `r"a#b"` is flat, `r"""a#b"""` carries a `co`. The
        // delimiter's length is the whole of the difference.
        if verbose && rest.starts_with('#') {
            let end = rest.find(delimiter).unwrap_or(rest.len());
            push(out, Class::Comment, &rest[..end]);
            at += end;
            continue;
        }
        if let Some(after) = rest.strip_prefix('\\') {
            let width = 1 + after.chars().next().map_or(0, char::len_utf8);
            let escape = if after.starts_with(|c: char| c.is_ascii_alphabetic()) {
                Class::DecVal
            } else {
                Class::Char
            };
            push(out, escape, &rest[..width]);
            at += width;
            continue;
        }
        // An `fr"…"` is both: `fr"{x}\d"` keeps the `sc` braces of an
        // f-string *and* reads `\d` as a regexp escape.
        if placeholders
            && rest.starts_with('{')
            && let Some(end) = rest.find('}')
        {
            push(out, Class::SpecialChar, "{");
            push(out, Class::Normal, &rest[1..end]);
            push(out, Class::SpecialChar, "}");
            at += end + 1;
            continue;
        }
        if rest.starts_with('[') {
            at += character_class(rest, out);
            continue;
        }
        if rest.starts_with('(') {
            at += group_opener(rest, &mut groups, out);
            continue;
        }
        if rest.starts_with(')') {
            match groups.pop().unwrap_or(Some(Class::Keyword)) {
                Some(closing) => push(out, closing, ")"),
                None => push(out, Class::Normal, ")"),
            }
            at += 1;
            continue;
        }
        if let Some(width) = quantifier(rest) {
            push(out, Class::Operator, &rest[..width]);
            at += width;
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        let single = match &rest[..width] {
            "|" => Class::ControlFlow,
            "." | "^" | "$" => Class::DecVal,
            _ => class,
        };
        push(out, single, &rest[..width]);
        at += width;
    }
    if verbose {
        let quote = delimiter.chars().next().unwrap_or('"');
        state.open_string = Some((quote, class, true));
    }
    at
}

/// `+`, `*`, `?`, or a repetition count — but only a **numeric** one:
/// `a{2,3}` quantifies and `a{b}` is three ordinary characters.
fn quantifier(rest: &str) -> Option<usize> {
    if rest.starts_with(['+', '*', '?']) {
        return Some(1);
    }
    let inside = rest.strip_prefix('{')?;
    let end = inside.find('}')?;
    let count = &inside[..end];
    let numeric = !count.is_empty()
        && count.bytes().all(|byte| byte.is_ascii_digit() || byte == b',')
        && count.bytes().filter(|byte| *byte == b',').count() <= 1;
    numeric.then_some(end + 2)
}

/// `[a-z]`, whose brackets and plain characters are `pp` while the
/// escapes inside keep their own classes. A `]` **immediately** after the
/// opening bracket, or after its `^`, is a literal — `[]]` is one class.
fn character_class(rest: &str, out: &mut Vec<(Class, String)>) -> usize {
    let opened = 1 + usize::from(rest[1..].starts_with('^'));
    let mut at = opened + usize::from(rest[opened..].starts_with(']'));
    let mut plain = 0;
    while at < rest.len() {
        if let Some(after) = rest[at..].strip_prefix('\\') {
            push(out, Class::Preprocessor, &rest[plain..at]);
            let width = 1 + after.chars().next().map_or(0, char::len_utf8);
            let escape = if after.starts_with(|c: char| c.is_ascii_alphabetic()) {
                Class::DecVal
            } else {
                Class::Char
            };
            push(out, escape, &rest[at..at + width]);
            at += width;
            plain = at;
            continue;
        }
        if rest[at..].starts_with(']') {
            push(out, Class::Preprocessor, &rest[plain..=at]);
            return at + 1;
        }
        at += rest[at..].chars().next().map_or(1, char::len_utf8);
    }
    // Unterminated: the rest of the run is still the class.
    push(out, Class::Preprocessor, &rest[plain..]);
    at
}

/// A `(`, and what its `)` will be. The plain group is `kw`; `(?: … )`
/// carries no class either side; a lookaround is `ex`; a named group is
/// `kw` with a `fu` for its name; and `(?i)`, `(?#…)` and `(?P=n)` are
/// each one span of their own.
fn group_opener(rest: &str, groups: &mut Vec<Option<Class>>, out: &mut Vec<(Class, String)>) -> usize {
    if let Some(end) = rest.find(')') {
        let whole = &rest[..=end];
        // `(?i)`, `(?ms)` — flags, and nothing else in the group.
        let flags = rest[1..end]
            .strip_prefix('?')
            .is_some_and(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_lowercase()));
        let alone = if rest.starts_with("(?#") {
            Some(Class::Comment)
        } else if rest.starts_with("(?P=") {
            Some(Class::Variable)
        } else if flags {
            Some(Class::Function)
        } else {
            None
        };
        if let Some(class) = alone {
            push(out, class, whole);
            return end + 1;
        }
    }
    if rest.starts_with("(?:") {
        groups.push(None);
        push(out, Class::Normal, "(?:");
        return 3;
    }
    if rest.starts_with("(?=") || rest.starts_with("(?!") {
        groups.push(Some(Class::Extension));
        push(out, Class::Extension, "(");
        push(out, Class::Function, &rest[1..3]);
        return 3;
    }
    if rest.starts_with("(?<") {
        groups.push(Some(Class::Keyword));
        push(out, Class::Keyword, "(");
        push(out, Class::Function, "?<");
        return 3;
    }
    if rest.starts_with("(?P<")
        && let Some(end) = rest.find('>')
    {
        groups.push(Some(Class::Keyword));
        push(out, Class::Keyword, "(");
        push(out, Class::Function, &rest[1..=end]);
        return end + 1;
    }
    groups.push(Some(Class::Keyword));
    push(out, Class::Keyword, "(");
    1
}

/// One escape, written and measured. Where the language is strict about
/// which escapes exist — python is — a backslash that begins none is an
/// `er`, and only the backslash: `"\d"` is `er` then `st`. Where it is
/// not, a run of backslashes is taken two at a time.
fn escaped(rest: &str, syntax: &Syntax, out: &mut Vec<(Class, String)>) -> usize {
    if syntax.has(STRICT_ESCAPES) {
        let Some(len) = python_escape(rest) else {
            push(out, Class::Error, "\\");
            return 1;
        };
        push(out, syntax.escape, &rest[..len]);
        return len;
    }
    let mut end = 0;
    while rest[end..].starts_with('\\') {
        end = (end + 2).min(rest.len());
    }
    push(out, syntax.escape, &rest[..end]);
    end
}

/// How long the escape at `rest` is, or `None` if the backslash begins
/// none. **Probed a to z, A to Z and 0 to 9**, and then form by form:
/// `\a \b \f \n \r \t \v` are escapes and every other letter alone is
/// not; one to three octal digits are; `\x` needs exactly two hex digits,
/// `\u` four and `\U` eight, and each is an error without them; `\N{…}`
/// is one whole escape; and of the punctuation only `\\`, `\'` and `\"`
/// count — `\.` and a backslash before a space are errors.
fn python_escape(rest: &str) -> Option<usize> {
    let after = rest.strip_prefix('\\')?;
    let first = after.chars().next()?;
    if "abfnrtv\\'\"\n".contains(first) {
        return Some(1 + first.len_utf8());
    }
    if ('0'..='7').contains(&first) {
        let digits = after.chars().take(3).take_while(|c| ('0'..='7').contains(c)).count();
        return Some(1 + digits);
    }
    let hex = |count: usize| {
        let digits = &after[1..];
        (digits.len() >= count && digits[..count].bytes().all(|b| b.is_ascii_hexdigit()))
            .then_some(2 + count)
    };
    match first {
        'x' => hex(2),
        'u' => hex(4),
        'U' => hex(8),
        'N' => after[1..].starts_with('{').then(|| after.find('}').map(|end| end + 2))?,
        _ => None,
    }
}

/// The quoted run that opens `rest`, if one does. A prefix letter belongs
/// to the string it introduces (`r"…"`), and `f` makes the run a `ss`
/// whose braces are `sc`.
fn opening(rest: &str, syntax: &Syntax) -> Option<Opener> {
    let first = rest.chars().next()?;
    if let Some((_, class)) = syntax.quotes.iter().find(|(q, _)| *q == first) {
        return Some(Opener { skip: 0, quote: first, class: *class, regexp: false, placeholders: false });
    }
    // A prefix is one or two letters — `r`, `b`, `f`, `u` and their
    // capitals, in either order: `rb"…"` and `br"…"` are both strings.
    let prefix: String = rest
        .chars()
        .take(2)
        .take_while(|c| syntax.string_prefixes.contains(*c))
        .collect();
    if prefix.is_empty() {
        return None;
    }
    let skip = prefix.len();
    let quote = rest[skip..].chars().next()?;
    syntax.quotes.iter().find(|(q, _)| *q == quote)?;
    // **A lowercase `r` makes the body a regular expression; a capital
    // `R` does not** — `r"\d"` carries a `dv` inside a `vs`, while
    // `R"\d"` is one flat `vs` with no tokens in it at all. Both are
    // `vs`; only the lowercase one is read. Measured, and not a
    // distinction anyone would invent.
    let raw = prefix.contains(['r', 'R']);
    let formatted = prefix.contains(['f', 'F']);
    let class = if raw {
        Class::VerbatimString
    } else if formatted {
        Class::SpecialString
    } else {
        Class::Str
    };
    Some(Opener {
        skip,
        quote,
        class,
        regexp: prefix.contains('r'),
        placeholders: formatted,
    })
}

fn quoted(
    text: &str,
    from: usize,
    opener: Opener,
    syntax: &Syntax,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
) -> usize {
    let Opener { skip, quote, class, regexp, placeholders } = opener;
    // `"""` and `'''` close on the same run of three, not on one.
    let triple: String = std::iter::repeat_n(quote, 3).collect();
    let delimiter = if text[from + skip..].starts_with(&triple) {
        triple.as_str()
    } else {
        &triple[..quote.len_utf8()]
    };
    push(out, class, &text[from..from + skip + delimiter.len()]);
    let mut at = from + skip + delimiter.len();
    if regexp {
        return python_regexp(text, at, delimiter, class, placeholders, state, out);
    }
    // A capital `R` is raw but unread: the body carries no tokens, so it
    // runs to the delimiter as one piece.
    if class == Class::VerbatimString {
        let end = text[at..].find(delimiter).map_or(text.len(), |index| at + index + delimiter.len());
        push(out, class, &text[at..end]);
        return end;
    }
    // Where an f-string placeholder's `!r` or `:>3` begins, if it has one.
    let mut spec: Option<usize> = None;
    while at < text.len() {
        let rest = &text[at..];
        if syntax.has(CONVERSIONS) && class == Class::Str
            && let Some(len) = specifier(rest, syntax.conversions)
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
            at += escaped(rest, syntax, out);
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
            spec = rest.find('}').and_then(|end| {
                rest[1..end].find([':', '!']).map(|index| at + 1 + index)
            });
            at += 1;
            continue;
        }
        if class == Class::SpecialString && rest.starts_with('}') {
            push(out, Class::SpecialChar, &rest[..1]);
            at += 1;
            continue;
        }
        // **A conversion or a format spec belongs to the placeholder**, not
        // to the expression: in `f"{x!r}"` the `!r}` is one `sc` run, as is
        // the `:>3}` of `f"{x:>3}"`. Only inside a placeholder — `spec` is
        // set when the `{` is read — so an ordinary `:` in the text is safe.
        if let Some(spec_at) = spec
            && spec_at == at
            && let Some(end) = rest.find('}')
        {
            push(out, Class::SpecialChar, &rest[..=end]);
            at += end + 1;
            spec = None;
            continue;
        }

        if class == Class::SpecialString
            && out.last().is_some_and(|(kind, run)| *kind == Class::SpecialChar && run.ends_with('{'))
        {
            // Bounded at the spec, so `!r` in `f"{x!r}"` is not read as
            // the operator it would be in ordinary code.
            let limit = spec.map_or(rest.len(), |spec_at| spec_at - at);
            at += interior(&rest[..limit], syntax, state, out);
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
        state.open_string = Some((quote, class, false));
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

/// Bash, which is not table-driven the way C and Python are: **its
/// classes are positional.** The first word of a command is a `fu` when
/// pandoc knows the command, a `bu` when it is a shell builtin and an
/// `ex` when it is neither; a word after it is an `at` when it starts
/// with `-`, and plain otherwise. `;`, `|`, `&&`, `||`, `&`, `{` and `}`
/// are `kw` that put the scanner back at command position, and so do
/// `if`, `then`, `else`, `elif`, `do`, `while` and `until` — but **not**
/// `for`, whose next word is the loop variable and stays plain.
///
/// Every one of those was measured, and so were the corners: `[` and `]`
/// are `bu` and turn `-x` between them into an `ot`; `name()` is one `fu`
/// including the parentheses; after `export`, `local`, `read` and their
/// kind a bare word is a `va`; `>&` is an `op` and the `2` after it a
/// `dv`.
fn bash(text: &str, state: &mut State, out: &mut Vec<(Class, String)>) {
    if let Some((delimiter, expands)) = state.heredoc.clone() {
        if text.trim() == delimiter {
            push(out, Class::Operator, text);
            state.heredoc = None;
            return;
        }
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            if expands && rest.starts_with('$') {
                at = bash_dollar(text, at, state, out);
                continue;
            }
            let stop = if expands { rest.find('$').unwrap_or(rest.len()) } else { rest.len() };
            push(out, Class::Str, &rest[..stop.max(1)]);
            at += stop.max(1);
        }
        return;
    }
    let mut from = 0;
    if state.open_string.is_some() {
        let Some(end) = text.find('\'').map(|index| index + 1) else {
            push(out, Class::Str, text);
            return;
        };
        push(out, Class::Str, &text[..end]);
        state.open_string = None;
        from = end;
    }
    if !text.trim_end().ends_with('\\') {
        // A line that does not continue starts a command on the next.
        bash_code(text, from, state, out, 0);
        if state.open_string.is_none() {
            // A pattern the `)` has not closed yet outlives the line.
            if state.position != Position::Pattern {
                state.position = Position::Command;
            }
            state.in_test = false;
        }
        return;
    }
    bash_code(text, from, state, out, 0);
}

/// One line of Ruby. The generic scanner gets keywords and numbers right
/// and nothing else: Ruby files its symbols under `wa`, its
/// single-quoted strings under `vs`, its instance variables under `ot`,
/// and splits capitalised names between `cn` and `dt` on whether they
/// hold a lowercase letter — `ABC` and `A_B` are constants, `Ab` and
/// `AbC` types. Every rule was read off the pinned binary one construct
/// at a time, **each in its own document**: a probe with one construct
/// per line puts an unterminated quote in the middle and every line
/// after it comes back a string.
/// One ruby word, and how much of the line it took — including the
/// trailing `?` or `!` that belongs to the name, and the `:` that
/// makes it a symbol where a `def` signature has not claimed one.
fn ruby_word(rest: &str, signature: &mut bool, out: &mut Vec<(Class, String)>) -> usize {
    let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    let mut end = word.len();
    // **A trailing `?` or `!` is part of the name**, not an operator after
    // it: `block_given?` and `exit!` are one `fu` each, and `.include?`
    // one `at`. Leaving them out put 62 stray `op` spans in twelve stdlib
    // files.
    if rest[end..].starts_with(['?', '!']) && !rest[end + 1..].starts_with('=') {
        end += 1;
    }
    let full = &rest[..end];
    let symbolic = rest[end..].starts_with(':') && !rest[end..].starts_with("::");
    // **A `def` signature suppresses exactly one symbol**, and then stops.
    // `def cp(a, b: 1, c: 2)` gives `b` an `op` and `c` a `wa`;
    // `def f; g(a: 1); end` gives `a` a `wa`, because the `;` ended the
    // signature; and `def self.cp(a, noop: nil)` gives `noop` a `wa`,
    // because `self` is not a method name. Every one of those four was
    // probed, and a simpler rule — "a `def` line has no symbols" — was
    // measured and was worse.
    if symbolic && *signature {
        *signature = false;
        push(out, class_of_word(full), full);
        return end;
    }
    if symbolic {
        push(out, Class::Warning, &rest[..=end]);
        return end + 1;
    }
    // `def` opens a signature; `self` right after it closes one, because
    // `def self.cp` names no method here.
    match full {
        "def" => *signature = true,
        "self" => *signature = false,
        _ => {}
    }
    push(out, class_of_word(full), full);
    end
}

/// One ruby word's class: the table first, then the shape of the name —
/// capitalised with a lowercase letter in it is a `dt`, capitalised
/// without one is a `cn`, anything else is plain.
fn class_of_word(word: &str) -> Class {
    if let Ok(index) = RUBY.keywords.binary_search_by_key(&word, |(name, _)| name) {
        return RUBY.keywords[index].1;
    }
    if word.starts_with(char::is_uppercase) {
        if word.contains(char::is_lowercase) { Class::DataType } else { Class::Constant }
    } else {
        Class::Normal
    }
}

/// A `=begin … =end` block, which takes whole lines and nothing less.
fn ruby_block_comment(text: &str, state: &mut State, out: &mut Vec<(Class, String)>) -> bool {
    if state.carried == Carried::BlockComment {
        push(out, Class::Comment, text);
        if text.starts_with("=end") {
            state.carried = Carried::Nothing;
        }
        return true;
    }
    if text.starts_with("=begin") {
        state.carried = Carried::BlockComment;
        push(out, Class::Comment, text);
        return true;
    }
    false
}

fn ruby(text: &str, state: &mut State, out: &mut Vec<(Class, String)>) {
    if ruby_block_comment(text, state, out) {
        return;
    }
    // Whether the scanner stands inside a `def`'s parameter list, where
    // the first `name:` is an argument rather than a symbol.
    let mut signature = false;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let byte = rest.as_bytes()[0];
        if byte == b';' {
            signature = false;
        }
        if byte == b'#' {
            push(out, Class::Comment, rest);
            return;
        }
        if byte.is_ascii_whitespace() {
            let run = rest.find(|c: char| !c.is_ascii_whitespace()).unwrap_or(rest.len());
            push(out, Class::Normal, &rest[..run]);
            at += run;
            continue;
        }
        if byte == b'"' {
            at = ruby_string(text, at, out);
            continue;
        }
        if byte == b'\'' {
            let end = rest[1..].find('\'').map_or(rest.len(), |index| index + 2);
            // **One character between single quotes is a `ch`**, two or
            // more a `vs`. Double quotes are `st` at every length.
            let quoted = &rest[..end];
            let class = if quoted.chars().count() == 3 {
                Class::Char
            } else {
                Class::VerbatimString
            };
            push(out, class, quoted);
            at += end;
            continue;
        }
        if byte == b'%'
            && let Some(run) = ruby_percent_literal(rest, out)
        {
            at += run;
            continue;
        }
        if let Some((class, run)) = ruby_sigil(rest) {
            push(out, class, &rest[..run]);
            at += run;
            continue;
        }
        if byte == b'[' || byte == b']' {
            push(out, Class::Keyword, &rest[..1]);
            at += 1;
            continue;
        }
        if byte == b'/' && ruby_expects_value(out) {
            at = ruby_regexp(text, at, out);
            continue;
        }
        if byte.is_ascii_digit() {
            at = ruby_number(text, at, out);
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            at += ruby_word(rest, &mut signature, out);
            continue;
        }
        if RUBY.operators.contains(byte as char) {
            let run = rest.find(|c: char| !RUBY.operators.contains(c)).unwrap_or(rest.len());
            push(out, Class::Operator, &rest[..run]);
            at += run;
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        push(out, Class::Normal, &rest[..width]);
        at += width;
    }
}

/// Write one percent literal, and say how much of the line it took.
fn ruby_percent_literal(rest: &str, out: &mut Vec<(Class, String)>) -> Option<usize> {
    let (body, open, close) = ruby_percent(rest)?;
    let opened = rest.len() - open.len();
    push(out, Class::Other, &rest[..=opened]);
    let inside = &open[1..];
    let end = inside.find(close).unwrap_or(inside.len());
    push(out, body, &inside[..end]);
    let closed = end < inside.len();
    if closed {
        push(out, Class::Other, &inside[end..=end]);
    }
    Some(opened + 1 + end + usize::from(closed))
}

/// A percent literal — `%w[a b]`, `%q(x)`, `%r{re}` — as the class its
/// body takes, the text from its opening delimiter, and the character
/// that closes it.
///
/// **The letter decides whether the body interpolates**, and the answers
/// were probed rather than reasoned: the lowercase `q`, `w` and `i` give
/// a `vs`, their capitals and `r` and a bare `%` give an `st`, `%s` is a
/// `wa` and `%x` — a shell command — is an `in`. The delimiter may be any
/// punctuation; the bracketing four close with their partners and
/// everything else closes with itself.
///
/// A `%` followed by a letter or a space is the modulo operator, which is
/// what keeps `5 % 2` and `a%b` out of here.
fn ruby_percent(rest: &str) -> Option<(Class, &str, char)> {
    let after = &rest[1..];
    let (body, open) = match after.chars().next()? {
        letter @ ('q' | 'w' | 'i') => (Class::VerbatimString, &after[letter.len_utf8()..]),
        letter @ ('Q' | 'W' | 'I' | 'r') => (Class::Str, &after[letter.len_utf8()..]),
        's' => (Class::Warning, &after[1..]),
        'x' => (Class::Information, &after[1..]),
        _ => (Class::Str, after),
    };
    let delimiter = open.chars().next()?;
    if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '_' {
        return None;
    }
    let close = match delimiter {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        '<' => '>',
        same => same,
    };
    Some((body, open, close))
}

/// A name introduced by a sigil: `$global` and `@ivar` and `:symbol` and
/// `.method`. Each takes a trailing `?` or `!` where Ruby allows one.
fn ruby_sigil(rest: &str) -> Option<(Class, usize)> {
    let named = |from: usize| {
        let mut run = rest[from..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(rest.len(), |index| index + from);
        if rest[run..].starts_with(['?', '!']) && !rest[run + 1..].starts_with('=') {
            run += 1;
        }
        run
    };
    match rest.as_bytes().first()? {
        b'$' => {
            // `$!` and `$0` are globals too, so a sigil on its own still
            // takes the character after it.
            let run = named(1);
            Some((Class::Variable, if run == 1 { 2.min(rest.len()) } else { run }))
        }
        b'@' => Some((Class::Other, named(1 + usize::from(rest[1..].starts_with('@'))))),
        b':' if !rest.starts_with("::") => match named(1) {
            1 => None,
            run => Some((Class::Warning, run)),
        },
        // **`..` and `...` are ranges, not two attribute dots.**
        b'.' if rest.starts_with("..") => {
            Some((Class::Operator, if rest.starts_with("...") { 3 } else { 2 }))
        }
        b'.' if rest[1..].starts_with(|c: char| c.is_lowercase() || c == '_') => {
            Some((Class::Attribute, named(1)))
        }
        b'.' => Some((Class::Attribute, 1)),
        _ => None,
    }
}

/// Whether a `/` here opens a regexp rather than dividing.
///
/// **This is a literal set, not a notion of expression position.** Probed
/// against the binary: a regexp opens a line, and follows `=` `~` `(` `,`
/// `&` `|` `?` or one of the words below — but *not* `[`, `:`, `return`,
/// `case`, `next`, `break`, `yield`, `in`, or a bare method name, all of
/// which take the `op` reading. `[/re/]` really does come back `op`.
fn ruby_expects_value(out: &[(Class, String)]) -> bool {
    /// Keywords a regexp may follow. `if` and `unless` are here; `case`
    /// and `return`, which read the same way to a person, are not.
    const OPENING: &[&str] = &[
        "and", "begin", "do", "elsif", "if", "not", "or", "rescue", "then", "unless", "until",
        "when", "while",
    ];
    let Some((class, text)) = out.iter().rev().find(|(_, text)| !text.trim().is_empty()) else {
        return true;
    };
    match class {
        Class::Operator => text.ends_with(['=', '~', '&', '|', '?']),
        Class::ControlFlow | Class::Keyword => OPENING.contains(&text.as_ref()),
        Class::Normal => text.ends_with(['(', ',']),
        _ => false,
    }
}

/// A regexp: `ss` throughout, save escapes, which are `sc`. Trailing
/// flags belong to the literal — `/a/i` is one `ss` run.
fn ruby_regexp(text: &str, from: usize, out: &mut Vec<(Class, String)>) -> usize {
    let mut at = from + 1;
    let mut start = from;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with('\\') {
            let width = 2.min(rest.len());
            push(out, Class::SpecialString, &text[start..at]);
            push(out, Class::SpecialChar, &rest[..width]);
            at += width;
            start = at;
            continue;
        }
        if let Some(after) = rest.strip_prefix('/') {
            let flags = after.find(|c: char| !c.is_ascii_alphabetic()).map_or(rest.len(), |i| i + 1);
            push(out, Class::SpecialString, &text[start..at + flags]);
            return at + flags;
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    // Unterminated: the rest of the line is still the literal.
    push(out, Class::SpecialString, &text[start..]);
    at
}

/// A double-quoted run, whose `#{ … }` marks are `sc` and whose contents
/// between them are ordinary code.
fn ruby_string(text: &str, from: usize, out: &mut Vec<(Class, String)>) -> usize {
    push(out, Class::Str, "\"");
    let mut at = from + 1;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with('"') {
            push(out, Class::Str, "\"");
            return at + 1;
        }
        if rest.starts_with('\\') {
            push(out, Class::SpecialChar, &rest[..2.min(rest.len())]);
            at += 2.min(rest.len());
            continue;
        }
        if rest.starts_with("#{") {
            push(out, Class::SpecialChar, "#{");
            let end = rest.find('}').unwrap_or(rest.len());
            // **The inside of `#{ … }` is code**, and comes back with its
            // own classes: `"#{@addr}"` carries an `ot`, not a run of
            // plain text. A fresh state, because nothing an interpolation
            // opens can outlive the string it sits in.
            ruby(&rest[2..end], &mut State::default(), out);
            if end < rest.len() {
                push(out, Class::SpecialChar, "}");
            }
            at += end + 1;
            continue;
        }
        let stop = rest.find(['"', '\\', '#']).unwrap_or(rest.len());
        push(out, Class::Str, &rest[..stop.max(1)]);
        at += stop.max(1);
    }
    at
}

/// A number: `dv` plainly, `fl` with a point or an exponent, `bn` in any
/// base but ten. `_` groups digits and belongs to the number.
fn ruby_number(text: &str, from: usize, out: &mut Vec<(Class, String)>) -> usize {
    let rest = &text[from..];
    if rest.starts_with("0x") || rest.starts_with("0b") || rest.starts_with("0o") {
        let run = rest[2..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map_or(rest.len(), |index| index + 2);
        push(out, Class::BaseN, &rest[..run]);
        return from + run;
    }
    let mut run = rest.find(|c: char| !c.is_ascii_digit() && c != '_').unwrap_or(rest.len());
    let mut float = false;
    if rest[run..].starts_with('.') && rest[run + 1..].starts_with(|c: char| c.is_ascii_digit()) {
        float = true;
        run += 1 + rest[run + 1..]
            .find(|c: char| !c.is_ascii_digit() && c != '_')
            .unwrap_or(rest.len() - run - 1);
    }
    if rest[run..].starts_with(['e', 'E']) {
        float = true;
        let mut end = run + 1;
        if rest[end..].starts_with(['+', '-']) {
            end += 1;
        }
        run = end + rest[end..].find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len() - end);
    }
    push(out, if float { Class::Float } else { Class::DecVal }, &rest[..run]);
    from + run
}

/// Words after which a bare word is a variable name rather than a value.
fn names_variables(word: &str) -> bool {
    matches!(
        word,
        "export" | "local" | "declare" | "readonly" | "typeset" | "read" | "unset"
    )
}

/// Words that put the scanner back at command position.
fn resumes_command(word: &str) -> bool {
    matches!(
        word,
        "if" | "then" | "else" | "elif" | "do" | "while" | "until" | "!" | "break" | "continue"
    )
}

/// Scan bash code from `at`, stopping at an unmatched `)` when `depth`
/// is above zero — which is how a `$( … )` substitution ends.
/// Where in a command the bash scanner stands. A word is classified
/// only at the start of a command; between a `case` label's `;;` and
/// its `)` it is a pattern instead.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    Command,
    Pattern,
    Word,
}

/// What one line of bash has said so far — none of it survives the line.
#[derive(Default)]
struct Cursor {
    /// A naming word (`export`, `read`) makes the bare words after it
    /// variable names rather than values.
    naming: bool,
    /// The scanner is in the value of a `name=`, which is plain text.
    valued: bool,
    /// That value is a `name=( … )` array, so its whitespace does not
    /// end it and its parentheses belong to the name.
    in_array: bool,
    /// Parentheses opened on this line: a `$( ( … ) | cmd )` closes on
    /// its own, and one left open at the end closes on a later line.
    open: usize,
}

/// Scan bash code from `at`, stopping at an unmatched `)` when `depth`
/// is above zero — which is how a `$( … )` substitution ends.
fn bash_code(
    text: &str,
    from: usize,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
    depth: usize,
) -> usize {
    let mut at = from;
    let mut cursor = Cursor::default();
    while at < text.len() {
        let rest = &text[at..];
        if depth > 0 && cursor.open == 0 && state.parens == 0 && rest.starts_with(')') {
            return at;
        }
        let byte = rest.as_bytes()[0];
        if rest.starts_with("esac") && !rest[4..].starts_with(|c: char| c.is_alphanumeric()) {
            state.position = Position::Command;
        }
        if state.position == Position::Pattern && !byte.is_ascii_whitespace() && byte != b'#' {
            at = bash_pattern(text, at, state, out);
            continue;
        }
        if byte.is_ascii_whitespace() {
            let run = rest.find(|c: char| !c.is_ascii_whitespace()).unwrap_or(rest.len());
            push(out, Class::Normal, &rest[..run]);
            at += run;
            if cursor.valued && !cursor.in_array {
                cursor.valued = false;
                state.position = if cursor.naming { Position::Word } else { Position::Command };
            }
            continue;
        }
        if byte == b'#' && (out.is_empty() || out.last().is_some_and(|(_, r)| r.ends_with(' '))) {
            push(out, Class::Comment, rest);
            return text.len();
        }
        // **Inside a `name=( … )` array, `[key]=` is a subscript**, not a
        // glob: the brackets are `op`, a quoted key keeps its `st`, a
        // bare one carries nothing, and the `=` after the `]` is a `va`.
        if cursor.in_array
            && let Some(run) = array_key(rest, out)
        {
            at += run;
            continue;
        }
        if let Some(next) = bash_punctuation(text, at, state, out, depth) {
            at = next;
            continue;
        }
        // **`&>` and `&>>` redirect; they do not end the command.** The
        // `&` of one was being read as the separator, which put the
        // scanner back at command position and made `/dev/null` an `ex`.
        if let Some(after) = rest.strip_prefix("&>") {
            let run = 2 + usize::from(after.starts_with('>'));
            push(out, Class::Operator, &rest[..run]);
            state.position = Position::Word;
            at += run;
            continue;
        }
        // The operators that end a command and start another.
        let ends_command = ["&&", "||", ";;", ";", "|", "&"]
            .into_iter()
            .find(|word| rest.starts_with(word));
        if let Some(word) = ends_command {
            let class = if word == ";;" { Class::ControlFlow } else { Class::Keyword };
            push(out, class, word);
            state.position = if word == ";;" && state.in_case {
                Position::Pattern
            } else {
                Position::Command
            };
            cursor.naming = false;
            at += word.len();
            continue;
        }
        // **A backtick is a command substitution**, and pandoc marks both
        // of its ticks `kw` with a command between them. Nothing here
        // read them at all, so `x=`date`` lost the whole run.
        if rest.starts_with('`') {
            at = backticked(text, at, state, out, depth);
            continue;
        }
        if rest.starts_with('=') {
            let assigns =
                cursor.valued || out.last().is_some_and(|(class, _)| *class == Class::Attribute);
            let class = match (state.in_test, assigns) {
                (true, _) => Class::Other,
                (false, true) => Class::Operator,
                (false, false) => Class::Normal,
            };
            push(out, class, "=");
            at += 1;
            continue;
        }
        if byte.is_ascii_digit() {
            let run = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            if returning(out) || rest[run..].starts_with(['>', '<']) {
                push(out, Class::DecVal, &rest[..run]);
                at += run;
                continue;
            }
        }
        let word: String = rest
            .chars()
            // A backtick ends a word: it closes the command substitution
            // it opened, and `date` inside one is a command like any
            // other. Without it here, `` `date` `` was one `ex`.
            .take_while(|c| !c.is_ascii_whitespace() && !"'\"$;|&<>=()#\\*?`".contains(*c))
            .collect();
        if word.is_empty() {
            at += bash_paren(rest, state, &mut cursor, out);
            continue;
        }
        at += word.len();
        at = bash_word(text, at, &word, state, &mut cursor, out);
    }
    state.parens += cursor.open;
    text.len()
}

/// A `` ` … ` `` substitution: both ticks are `kw`, and what lies between
/// them is a command. Nothing read them at all before, so `` x=`date` ``
/// lost the whole run.
fn backticked(
    text: &str,
    at: usize,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
    depth: usize,
) -> usize {
    push(out, Class::Keyword, "`");
    let was = (state.position, state.in_test);
    state.position = Position::Command;
    state.in_test = false;
    let end = bash_code(text, at + 1, state, out, depth + 1);
    (state.position, state.in_test) = was;
    if text[end..].starts_with('`') {
        push(out, Class::Keyword, "`");
        return end + 1;
    }
    end
}

/// A `[key]=` inside a `name=( … )` array, and how much of the line it
/// took. The brackets are `op`, a quoted key keeps its `st`, a bare one
/// carries nothing, and the `=` after the `]` is a `va` — not the `op` an
/// assignment outside an array gets.
fn array_key(rest: &str, out: &mut Vec<(Class, String)>) -> Option<usize> {
    let close = rest.strip_prefix('[')?.find(']')? + 1;
    push(out, Class::Operator, "[");
    let key = &rest[1..close];
    let class = if key.starts_with(['"', '\'']) { Class::Str } else { Class::Normal };
    push(out, class, key);
    push(out, Class::Operator, "]");
    let assigns = rest[close + 1..].starts_with('=');
    if assigns {
        push(out, Class::Variable, "=");
    }
    Some(close + 1 + usize::from(assigns))
}

/// The punctuation that reads the same wherever it stands: quotes,
/// substitutions, redirections and globs. `None` if `at` is not one.
fn bash_punctuation(
    text: &str,
    at: usize,
    state: &mut State,
    out: &mut Vec<(Class, String)>,
    depth: usize,
) -> Option<usize> {
    let rest = &text[at..];
    let byte = rest.as_bytes()[0];
    if byte == b'\\' {
        let width = 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
        push(out, Class::DataType, &rest[..width]);
        return Some(at + width);
    }
    if byte == b'\'' {
        let Some(end) = rest[1..].find('\'').map(|index| index + 2) else {
            push(out, Class::Str, rest);
            state.open_string = Some(('\'', Class::Str, false));
            return Some(text.len());
        };
        push(out, Class::Str, &rest[..end]);
        state.position = Position::Word;
        return Some(at + end);
    }
    if byte == b'"' || byte == b'$' {
        let end = if byte == b'"' {
            bash_string(text, at, state, out)
        } else {
            bash_dollar(text, at, state, out)
        };
        state.position = Position::Word;
        return Some(end);
    }
    if rest.starts_with("<<") && !rest.starts_with("<<<") {
        return Some(at + heredoc(rest, state, out));
    }
    if rest.starts_with("<(") || rest.starts_with(">(") {
        push(out, Class::Operator, &rest[..2]);
        let was = state.position;
        state.position = Position::Command;
        let mut end = bash_code(text, at + 2, state, out, depth + 1);
        state.position = was;
        if text[end..].starts_with(')') {
            push(out, Class::Operator, ")");
            end += 1;
        }
        return Some(end);
    }
    if rest.starts_with(['>', '<']) {
        let run = rest.find(|c: char| !">&<".contains(c)).unwrap_or(rest.len());
        push(out, Class::Operator, &rest[..run]);
        let digits =
            text[at + run..].find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len() - at - run);
        push(out, Class::DecVal, &text[at + run..at + run + digits]);
        return Some(at + run + digits);
    }
    if byte == b'*' || byte == b'?' {
        let run = rest.find(|c: char| c != '*' && c != '?').unwrap_or(rest.len());
        push(out, Class::Preprocessor, &rest[..run]);
        return Some(at + run);
    }
    // `[0-9]` globs; a `[` with a space after it is the test builtin.
    let bracket = rest
        .strip_prefix('[')
        .and_then(|tail| tail.find(']'))
        .filter(|end| !rest[1..=*end].contains(char::is_whitespace))?;
    push(out, Class::Preprocessor, "[");
    for piece in rest[1..=bracket].split_inclusive('-') {
        push(out, Class::SpecialString, piece.trim_end_matches('-'));
        if piece.ends_with('-') {
            push(out, Class::Preprocessor, "-");
        }
    }
    push(out, Class::Preprocessor, "]");
    state.position = Position::Word;
    Some(at + bracket + 2)
}

/// A `(` or `)`, which either groups commands or holds an array.
fn bash_paren(
    rest: &str,
    state: &mut State,
    cursor: &mut Cursor,
    out: &mut Vec<(Class, String)>,
) -> usize {
    let width = rest.chars().next().map_or(1, char::len_utf8);
    // **A bare `(( … ))` evaluates arithmetic**, where a bare word is a
    // `va` and the numbers are numbers — the same rules as `$(( … ))`,
    // but closing on a `kw` rather than a `va`. Without this, `if ((
    // verboselevel >= t ))` called its variables `ex`.
    if rest.starts_with("((") && state.position == Position::Command {
        push(out, Class::Keyword, "((");
        return bash_arith(rest, 2, Class::Keyword, out);
    }
    if &rest[..width] == "(" {
        let valued = cursor.valued;
        push(out, if valued { Class::Variable } else { Class::Keyword }, "(");
        cursor.in_array = valued;
        state.position = if valued { Position::Word } else { Position::Command };
        cursor.open += 1;
    } else if &rest[..width] == ")" {
        // A `)` closes the innermost thing still open: a group from this
        // line, one from an earlier line, or the `$(` of a substitution,
        // whose parentheses are its own.
        let class = if cursor.open > 0 {
            cursor.open -= 1;
            if cursor.in_array { Class::Variable } else { Class::Keyword }
        } else if state.parens > 0 {
            state.parens -= 1;
            Class::Keyword
        } else if state.subst > 0 {
            state.subst -= 1;
            Class::Variable
        } else {
            Class::Keyword
        };
        push(out, class, ")");
        if cursor.in_array {
            cursor.in_array = false;
            cursor.valued = false;
            state.position = Position::Command;
        }
    } else {
        push(out, Class::Normal, &rest[..width]);
    }
    width
}

/// The inside of an array subscript. A numeric index is a `dv`; a name or
/// a sum carries no class at all, which is measured rather than assumed —
/// `${a[foo]}` and `${a[i+1]}` both come back bare.
fn bash_subscript(inside: &str, out: &mut Vec<(Class, String)>) {
    if !inside.is_empty() && inside.bytes().all(|byte| byte.is_ascii_digit()) {
        push(out, Class::DecVal, inside);
    } else {
        bash_expanded(inside, Class::Normal, out);
    }
}

/// The name after a `function` keyword, **with the space before it** —
/// `function f() {` is `kw|function` then `fu| f()`, parens and all.
fn named_function(after: &str, out: &mut Vec<(Class, String)>) -> usize {
    let space = after.find(|c: char| !c.is_ascii_whitespace()).unwrap_or(after.len());
    let named = after[space..]
        .find(|c: char| !c.is_alphanumeric() && !"_-.:".contains(c))
        .map_or(after.len(), |index| space + index);
    let named = named + usize::from(after[named..].starts_with("()")) * 2;
    push(out, Class::Function, &after[..named]);
    named
}

/// One word, classified by where it stands. `at` has already passed it.
fn bash_word(
    text: &str,
    at: usize,
    word: &str,
    state: &mut State,
    cursor: &mut Cursor,
    out: &mut Vec<(Class, String)>,
) -> usize {
    let commanding = state.position == Position::Command;
    // **`name[index]=` assigns to one slot of an array.** The name is a
    // `va`, the brackets are `op` with the index tokenized between them,
    // and the `]` merges with the `=` that follows. Taken before the word
    // is classified, because the word scan stops at the `$` of `a[$i]=1`
    // and leaves `a[` looking like a command.
    if (commanding || cursor.naming)
        && let Some(open) = word.find('[')
        && !word[..open].is_empty()
        && let Some(close) = text[at - word.len() + open..].find(']')
    {
        let from = at - word.len() + open;
        let after = &text[from + close + 1..];
        if after.starts_with('=') || after.starts_with("+=") {
            push(out, Class::Variable, &word[..open]);
            push(out, Class::Operator, "[");
            bash_subscript(&text[from + 1..from + close], out);
            // `]+=` is one operator run, so the `+` of an append goes
            // here rather than being left for the scan.
            let appends = after.starts_with("+=");
            push(out, Class::Operator, if appends { "]+" } else { "]" });
            state.position = Position::Word;
            cursor.valued = true;
            return from + close + 1 + usize::from(appends);
        }
    }
    // **`function name` names a function, and the space belongs to the
    // name**: `function f() {` is `kw|function` then `fu| f()`, parens
    // and all. Without this the name carried no class at all.
    if commanding && word == "function" {
        push(out, Class::Keyword, word);
        state.position = Position::Command;
        return at + named_function(&text[at..], out);
    }
    // `name()` is one piece, and it is the name of a function.
    if commanding && text[at..].starts_with("()") {
        push(out, Class::Function, &format!("{word}()"));
        return at + 2;
    }
    if word == "!" && state.in_test {
        if text[at..].starts_with('=') {
            push(out, Class::Other, "!=");
            return at + 1;
        }
        push(out, Class::Other, "!");
        return at;
    }
    // `name=` names a variable; what follows the `=` is its value.
    let appends = word.ends_with('+') && text[at..].starts_with('=');
    if (commanding || cursor.naming) && (text[at..].starts_with('=') || appends) {
        push(out, Class::Variable, word.trim_end_matches('+'));
        if appends {
            push(out, Class::Operator, "+");
        }
        state.position = Position::Word;
        cursor.valued = true;
        return at;
    }
    if cursor.naming && !cursor.valued && !word.starts_with('-') {
        push(out, Class::Variable, word);
        return at;
    }
    if word == "{" || word == "}" {
        push(out, Class::Keyword, word);
        state.position = Position::Command;
        cursor.naming = false;
        return at;
    }
    if word == "[" || word == "]" || word == "[[" || word == "]]" {
        // **The doubled brackets are `kw`, the single ones `bu`.** Probed:
        // `[[ -f x ]]` comes back kw/ot/kw, `[ -f x ]` bu/ot/bu. Calling
        // all four `bu` was the single largest divergence on real scripts.
        let class = if word.len() == 2 { Class::Keyword } else { Class::BuiltIn };
        push(out, class, word);
        state.in_test = word.starts_with('[');
        state.position = Position::Word;
        return at;
    }
    if commanding && word == "!" {
        // `! cmd` negates the command, and takes the space with it.
        let run = text[at..].find(|c: char| !c.is_ascii_whitespace()).unwrap_or(0);
        push(out, Class::Other, &text[at - 1..at + run]);
        return at + run;
    }
    if commanding {
        let class = BASH
            .keywords
            .binary_search_by_key(&word, |(name, _)| name)
            .map_or(Class::Extension, |index| BASH.keywords[index].1);
        push(out, class, word);
        cursor.naming = names_variables(word);
        state.position = if resumes_command(word) { Position::Command } else { Position::Word };
        if word == "case" {
            state.in_case = true;
        } else if word == "esac" {
            state.in_case = false;
        }
        return at;
    }
    let opens = at == word.len() || text[..at - word.len()].ends_with(char::is_whitespace);
    if opens && word.starts_with('-') {
        push(out, if state.in_test { Class::Other } else { Class::Attribute }, word);
        return at;
    }
    if word == "in" {
        push(out, Class::Keyword, word);
        state.position = if state.in_case { Position::Pattern } else { state.position };
        return at;
    }
    push(out, Class::Normal, word);
    at
}

/// Whether the pieces so far end in a `return`, whose bare number is the
/// one bare number bash reads as a number.
fn returning(out: &[(Class, String)]) -> bool {
    out.iter()
        .rev()
        .find(|(_, run)| !run.trim().is_empty())
        .is_some_and(|(class, run)| *class == Class::ControlFlow && run == "return")
}

/// A double-quoted run: the quotes and the text are `st`, a `$…` inside
/// is a `va`, and a `$( … )` inside is code again.
fn bash_string(text: &str, from: usize, state: &mut State, out: &mut Vec<(Class, String)>) -> usize {
    push(out, Class::Str, "\"");
    let mut at = from + 1;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with('"') {
            push(out, Class::Str, "\"");
            return at + 1;
        }
        if rest.starts_with('$') {
            at = bash_dollar(text, at, state, out);
            continue;
        }
        if rest.starts_with('\\') && rest[1..].starts_with(['"', '$', '\\', '`']) {
            push(out, Class::DataType, &rest[..2]);
            at += 2;
            continue;
        }
        if rest.starts_with('\\') {
            push(out, Class::Str, &rest[..2.min(rest.len())]);
            at += 2.min(rest.len());
            continue;
        }
        let stop = rest.find(['"', '$', '\\']).unwrap_or(rest.len());
        push(out, Class::Str, &rest[..stop.max(1)]);
        at += stop.max(1);
    }
    at
}

/// `$name`, `${…}` and `$( … )`, all of which are `va` — with the
/// substitution's contents read as code and its `:-` and friends as `op`.
fn bash_dollar(text: &str, from: usize, state: &mut State, out: &mut Vec<(Class, String)>) -> usize {
    let rest = &text[from..];
    if rest.starts_with("$'") {
        push(out, Class::Str, "$'");
        let mut at = from + 2;
        while at < text.len() {
            let tail = &text[at..];
            if tail.starts_with('\'') {
                push(out, Class::Str, "'");
                return at + 1;
            }
            if tail.starts_with('\\') {
                push(out, Class::DataType, &tail[..2.min(tail.len())]);
                at += 2.min(tail.len());
                continue;
            }
            let stop = tail.find(['\'', '\\']).unwrap_or(tail.len());
            push(out, Class::Str, &tail[..stop]);
            at += stop;
        }
        return at;
    }
    if rest.starts_with("$((") {
        push(out, Class::Variable, "$((");
        return bash_arith(text, from + 3, Class::Variable, out);
    }
    if rest.starts_with("$(") {
        push(out, Class::Variable, "$(");
        let was = (state.position, state.in_test);
        state.position = Position::Command;
        state.in_test = false;
        state.subst += 1;
        let end = bash_code(text, from + 2, state, out, 1);
        (state.position, state.in_test) = was;
        if text[end..].starts_with(')') {
            push(out, Class::Variable, ")");
            state.subst -= 1;
            return end + 1;
        }
        return end;
    }
    if rest.starts_with("${") {
        let close = rest.find('}').unwrap_or(rest.len() - 1);
        push(out, Class::Variable, "${");
        bash_braced(&rest[2..close], out);
        push(out, Class::Variable, "}");
        return from + close + 1;
    }
    let len = rest[1..]
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(rest.len(), |index| index + 1);
    let len = if len == 1 { 2.min(rest.len()) } else { len };
    push(out, Class::Variable, &rest[..len]);
    from + len
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
        // **A docstring holds no conversions and no placeholders**: it is
        // prose, and pandoc leaves `%d` and `{name}` in it alone. The
        // first line of one already knew that, because `quoted` asks
        // whether the class is `st`; every line after it did not, and
        // `difflib.py` is full of `%d` inside `""" … """`.
        if class != Class::Documentation
            && syntax.has(CONVERSIONS)
            && let Some(len) = specifier(rest, syntax.conversions)
        {
            push(out, Class::SpecialChar, &rest[..len]);
            at += len;
            continue;
        }
        if class != Class::Documentation
            && syntax.has(PLACEHOLDERS)
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
fn specifier(rest: &str, letters: &str) -> Option<usize> {
    let rest = rest.strip_prefix('%')?;
    // Python's `%(name)s` names its argument, and the whole of it is one
    // `sc`. C has nothing of the kind, so the parenthesis is only skipped
    // where the language's letters include python's `r`.
    let mut at = if letters.contains('r') && rest.starts_with('(') {
        rest.find(')')? + 1
    } else {
        0
    };
    for (index, c) in rest[at..].char_indices() {
        if "-+ #0123456789.*hlLqjzt".contains(c) {
            continue;
        }
        at += index;
        return letters.contains(c).then(|| 1 + at + c.len_utf8());
    }
    None
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

/// The inside of a `$(( … ))`, where a bare name is a variable and the
/// numbers are numbers.
fn bash_arith(text: &str, from: usize, closing: Class, out: &mut Vec<(Class, String)>) -> usize {
    let mut at = from;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("))") {
            push(out, closing, "))");
            return at + 2;
        }
        if rest.starts_with("${") {
            at = bash_dollar(text, at, &mut State::default(), out);
            continue;
        }
        let byte = rest.as_bytes()[0];
        let run = if byte.is_ascii_whitespace() {
            rest.find(|c: char| !c.is_ascii_whitespace()).unwrap_or(rest.len())
        } else {
            rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(rest.len())
        };
        if run == 0 {
            push(out, Class::Operator, &rest[..1]);
            at += 1;
            continue;
        }
        let word = &rest[..run];
        let class = if byte.is_ascii_whitespace() {
            Class::Normal
        } else if word.starts_with("0x") || word.starts_with("0X") {
            Class::BaseN
        } else if byte.is_ascii_digit() {
            Class::DecVal
        } else {
            Class::Variable
        };
        push(out, class, word);
        at += run;
    }
    at
}

/// The inside of a `${ … }`, which names a variable and then does one
/// thing to it — measure it, default it, trim it or replace within it.
fn bash_braced(inner: &str, out: &mut Vec<(Class, String)>) {
    // `${#name}` measures; every other form names first and operates after.
    let inner = match inner.strip_prefix('#') {
        Some(rest) if !rest.is_empty() => {
            push(out, Class::Operator, "#");
            rest
        }
        _ => inner,
    };
    let cut = inner.find([':', '-', '=', '+', '?', '#', '%', '/', '[']).unwrap_or(inner.len());
    push(out, Class::Variable, &inner[..cut]);
    let tail = &inner[cut..];
    let Some(head) = tail.bytes().next() else {
        return;
    };
    if head == b'[' {
        let end = tail.find(']').map_or(tail.len(), |index| index + 1);
        // **A subscript is an expression, not part of the bracket.**
        // `${a[$i]}` carries a `va` between two `op`, and `${a["$k"]}` a
        // quoted run — only `[@]` and `[*]`, which name no index, come
        // back as one operator.
        let inside = tail[1..end].strip_suffix(']').unwrap_or(&tail[1..end]);
        if inside == "@" || inside == "*" {
            push(out, Class::Operator, &tail[..end]);
        } else {
            push(out, Class::Operator, "[");
            bash_subscript(inside, out);
            push(out, Class::Operator, "]");
        }
        bash_braced(&tail[end..], out);
        return;
    }
    if head == b'/' {
        // `${name/pattern/replacement}`
        let op = 1 + usize::from(tail[1..].starts_with('/'));
        push(out, Class::Operator, &tail[..op]);
        let end = tail[op..].find('/').map_or(tail.len(), |index| op + index);
        bash_expanded(&tail[op..end], Class::SpecialString, out);
        push(out, Class::Operator, &tail[end..(end + 1).min(tail.len())]);
        bash_expanded(&tail[(end + 1).min(tail.len())..], Class::Normal, out);
        return;
    }
    let op = if head == b':' {
        1 + usize::from(tail[1..].starts_with(['-', '=', '+', '?']))
    } else if b"-=+?".contains(&head) {
        1
    } else {
        usize::from(tail[1..].starts_with(head as char)) + 1
    };
    push(out, Class::Operator, &tail[..op]);
    bash_expanded(&tail[op..], Class::Normal, out);
}

/// Plain text that still expands variables and honours escapes — the
/// halves of a `${name/pattern/replacement}` and the like.
fn bash_expanded(text: &str, plain: Class, out: &mut Vec<(Class, String)>) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with('\\') {
            push(out, Class::DataType, &rest[..2.min(rest.len())]);
            at += 2.min(rest.len());
            continue;
        }
        if let Some(name) = rest.strip_prefix('$') {
            let run = name
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map_or(rest.len(), |index| index + 1);
            push(out, Class::Variable, &rest[..run.max(1)]);
            at += run.max(1);
            continue;
        }
        if rest.starts_with('"') {
            push(out, Class::Str, "\"");
            at += 1;
            continue;
        }
        if rest.starts_with(['*', '?']) {
            let run = rest.find(|c: char| c != '*' && c != '?').unwrap_or(rest.len());
            push(out, Class::Preprocessor, &rest[..run]);
            at += run;
            continue;
        }
        let stop = rest.find(['$', '\\', '*', '?', '"']).unwrap_or(rest.len());
        push(out, plain, &rest[..stop]);
        at += stop;
    }
}

/// One `case` pattern, up to the `)` that ends it. A pattern is an `ss`
/// but for the `*` and `?` that make it a pattern at all.
fn bash_pattern(text: &str, from: usize, state: &mut State, out: &mut Vec<(Class, String)>) -> usize {
    let rest = &text[from..];
    let byte = rest.as_bytes()[0];
    if byte == b'"' {
        return bash_string(text, from, state, out);
    }
    if byte == b'$' {
        return bash_dollar(text, from, state, out);
    }
    if byte == b'\'' {
        let end = rest[1..].find('\'').map_or(rest.len(), |index| index + 2);
        push(out, Class::Str, &rest[..end]);
        return from + end;
    }
    if byte == b')' {
        push(out, Class::Keyword, ")");
        state.position = Position::Command;
        return from + 1;
    }
    if byte == b'|' {
        push(out, Class::Keyword, "|");
        return from + 1;
    }
    if byte == b'*' || byte == b'?' {
        let run = rest.find(|c: char| c != '*' && c != '?').unwrap_or(rest.len());
        push(out, Class::Preprocessor, &rest[..run]);
        return from + run;
    }
    let run = rest
        .find(|c: char| "\"'$)|*?".contains(c) || c.is_ascii_whitespace())
        .unwrap_or(rest.len());
    push(out, Class::SpecialString, &rest[..run]);
    from + run
}

/// A `<<EOF` marker: the whole of it is one operator, and it opens a
/// here-document that the next line begins.
fn heredoc(rest: &str, state: &mut State, out: &mut Vec<(Class, String)>) -> usize {
    let mut end = 2 + usize::from(rest[2..].starts_with('-'));
    // **`cat << EOF` is the same as `cat <<EOF`**, and the space between
    // belongs to the operator: pandoc writes `op|<< EOF`. Without this
    // the heredoc never opened and its body was read as commands.
    end += rest[end..].find(|c: char| !c.is_ascii_whitespace()).unwrap_or(0);
    let quote = rest[end..].chars().next().filter(|c| *c == '\'' || *c == '"');
    if quote.is_some() {
        end += 1;
    }
    let word = end;
    end += rest[end..]
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len() - end);
    let delimiter = rest[word..end].to_owned();
    if quote.is_some_and(|quote| rest[end..].starts_with(quote)) {
        end += 1;
    }
    push(out, Class::Operator, &rest[..end]);
    if !delimiter.is_empty() {
        state.heredoc = Some((delimiter, quote.is_none()));
    }
    end
}

#[cfg(test)]
mod tests {
    use super::{Carried, Class, State, line};

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
        assert!(state.carried != Carried::BlockComment);
    }

    /// bash's rules, which are positional rather than lexical.
    #[test]
    fn bash_is_tokenized_by_position_rather_than_by_word() {
        let text = |pieces: &[(Class, &str)]| {
            pieces.iter().map(|(c, t)| (*c, (*t).to_owned())).collect::<Vec<_>>()
        };
        // The same word is a command in one place and a value in another.
        assert_eq!(
            classes("LANG=C sort file", "bash"),
            text(&[
                (Class::Variable, "LANG"),
                (Class::Operator, "="),
                (Class::Normal, "C "),
                (Class::Function, "sort"),
                (Class::Normal, " file"),
            ])
        );
        // A bare number is text; one beside a redirection is a number.
        assert_eq!(
            classes("echo hi >&2", "bash"),
            text(&[
                (Class::BuiltIn, "echo"),
                (Class::Normal, " hi "),
                (Class::Operator, ">&"),
                (Class::DecVal, "2"),
            ])
        );
        // A here-document is a string until its delimiter comes back,
        // and an unquoted delimiter still expands.
        let mut state = State::default();
        line("cat <<EOF", "bash", &mut state);
        assert_eq!(
            line("a $y", "bash", &mut state),
            text(&[(Class::Str, "a "), (Class::Variable, "$y")])
        );
        assert_eq!(line("EOF", "bash", &mut state), text(&[(Class::Operator, "EOF")]));
        // A `case` label is a pattern, and its `)` is not a `$( … )`'s.
        let mut state = State::default();
        line("case $x in", "bash", &mut state);
        assert_eq!(
            line("  a*) ls ;;", "bash", &mut state),
            text(&[
                (Class::Normal, "  "),
                (Class::SpecialString, "a"),
                (Class::Preprocessor, "*"),
                (Class::Keyword, ")"),
                (Class::Normal, " "),
                (Class::Function, "ls"),
                (Class::Normal, " "),
                (Class::ControlFlow, ";;"),
            ])
        );
    }

    /// Python's rules, which are a different order of hair from C's.
    #[test]
    fn python_is_tokenized_the_way_skylighting_tokenizes_it() {
        let text = |pieces: &[(Class, &str)]| {
            pieces.iter().map(|(c, t)| (*c, (*t).to_owned())).collect::<Vec<_>>()
        };
        // A string that opens a line is a docstring — a comment.
        // A docstring renders `co` and is **not** a `Comment`: alert
        // words are read in a comment and not in a docstring, so the two
        // cannot share a class. `\"\"\"TODO\"\"\"` carries no `al`.
        assert_eq!(classes("\"doc\"", "python"), text(&[(Class::Documentation, "\"doc\"")]));
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
