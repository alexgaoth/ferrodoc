//! Pandoc's template language, in the subset its own templates use.
//!
//! `-s` output is where "indistinguishable from pandoc" is most visible
//! and was furthest away: 174 of the 176 lines by which a standalone
//! page differed were pandoc's default template and its default
//! stylesheet. Those two files are vendored beside this module under the
//! BSD-3 option pandoc's `COPYRIGHT` offers for `data/templates`, so what
//! is left is rendering them the way pandoc does.
//!
//! **The subset is stated and the rest is refused by name.** Pandoc's
//! full template language has pipes (`$x/uppercase$`), nested field
//! access, `$it$`, and partials taking arguments; a template using one
//! gets an error saying which, never a page with a hole in it. What is
//! here is what pandoc's own `html5.html` and `styles.html` need, which
//! is also what almost every template in the wild uses:
//!
//! - `$var$`
//! - `$if(var)$ … $else$ … $endif$`
//! - `$for(var)$ … $sep$ … $endfor$`
//! - `$partial()$`
//! - `$$` for a literal `$`
//!
//! Two rules are easy to miss and both are load-bearing for byte
//! identity, so both have tests:
//!
//! - **a line holding nothing but a directive produces no line.**
//!   `$if(date-meta)$\n<p>…</p>\n$endif$` is one line of output, not
//!   three;
//! - **an indented variable indents every line of its value.** The
//!   default template has `    $styles.html()$`, and pandoc puts those
//!   four spaces in front of all 212 lines of the stylesheet.

use std::collections::HashMap;

/// What a template variable can hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string. Empty is *not* the same as absent: pandoc's `lang` is
    /// set and empty in a document with no language, and the template
    /// writes `lang=""` for it.
    Text(String),
    /// A list, for `$for(x)$`. An empty list is falsy, as pandoc has it.
    List(Vec<String>),
    /// Set with no value, for `$if(x)$`.
    Flag,
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            // Pandoc treats an empty string as false in `$if()$` — which
            // is why `$if(dir)$` writes nothing for a document with no
            // direction even though `dir` is set.
            Value::Text(text) => !text.is_empty(),
            Value::List(items) => !items.is_empty(),
            Value::Flag => true,
        }
    }

    fn as_text(&self) -> &str {
        match self {
            Value::Text(text) => text,
            // A list interpolated outside a `$for$` is its first item;
            // no template here does that, and returning "" would be a
            // hole rather than an error.
            Value::List(items) => items.first().map_or("", String::as_str),
            Value::Flag => "",
        }
    }
}

/// The variables a template is rendered against.
pub type Context = HashMap<String, Value>;

#[derive(Debug)]
enum Node {
    Text(String),
    /// A variable, and the indentation of the line it sits on.
    Var { name: String, indent: String },
    If { name: String, then: Vec<Node>, otherwise: Vec<Node> },
    For { name: String, body: Vec<Node>, separator: Vec<Node> },
    Partial { name: String, indent: String },
}

/// Render `template` against `context`, resolving `$partial()$` through
/// `partials`.
///
/// # Errors
///
/// A construct outside the supported subset, an unclosed `$if$`/`$for$`,
/// or a partial with no source — each named in the message.
pub fn render(
    template: &str,
    context: &Context,
    partials: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let nodes = parse(&tokenize(template)?, &mut 0, None)?;
    let mut out = String::with_capacity(template.len());
    emit(&nodes, context, partials, &mut out)?;
    Ok(out)
}

#[derive(Debug)]
enum Token {
    Text(String),
    /// `$…$`, with the indentation of the line it sits on.
    Directive { body: String, indent: String },
}

/// One directive as the scanner found it, before any trimming.
struct Found {
    body: String,
    indent: String,
    /// Text since the previous directive.
    before: String,
    /// Which line of the template it is on.
    line: usize,
    /// Nothing but whitespace before it on its line.
    starts_line: bool,
    /// Nothing but whitespace after it on its line.
    ends_line: bool,
}

/// Split into text and `$…$` directives, then decide which newlines a
/// control directive swallows.
///
/// **The rule is not "a line holding only a directive disappears".** It
/// is that a control directive swallows the whitespace before it and the
/// newline after it **when its construct spans more than one line** —
/// probed against the binary, where `A$if(x)$\nB\n$endif$C` renders as
/// `AC` (the newline after the opener is gone even though `A` precedes
/// it) while `$if(t)$<h1>$t$</h1>$endif$` on one line renders as an
/// *empty line*, newline intact. A conditional written inline is not a
/// line to be removed; one that opens a block is.
fn tokenize(template: &str) -> Result<Vec<Token>, String> {
    let (found, trailing) = scan(template)?;
    let multiline = spans_lines(&found)?;
    let mut tokens = Vec::new();
    for (index, directive) in found.iter().enumerate() {
        let mut before = directive.before.clone();
        if multiline[index] && directive.starts_line {
            before.truncate(before.trim_end_matches([' ', '\t']).len());
        }
        tokens.push(Token::Text(before));
        tokens.push(Token::Directive {
            body: directive.body.clone(),
            indent: directive.indent.clone(),
        });
    }
    // A directive's swallowed newline lives at the start of the *next*
    // token's text, and the last token is whatever followed the last
    // directive — or the whole template when it holds none.
    tokens.push(Token::Text(trailing));
    // Now swallow the newline after every multi-line control directive,
    // which is the first thing in the text that follows it.
    for index in 0..found.len() {
        if !(multiline[index] && found[index].ends_line) {
            continue;
        }
        let text_index = 2 * index + 2;
        if let Some(Token::Text(text)) = tokens.get_mut(text_index) {
            let trimmed = text.trim_start_matches([' ', '\t']);
            let rest = trimmed.strip_prefix('\n').unwrap_or(trimmed);
            *text = rest.to_owned();
        }
    }
    Ok(tokens)
}

/// The raw scan: every `$…$` with the text before it and where it sits.
fn scan(template: &str) -> Result<(Vec<Found>, String), String> {
    let chars: Vec<char> = template.chars().collect();
    let mut found: Vec<Found> = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let mut line = 1usize;
    let mut line_start = 0usize;
    while i < chars.len() {
        if chars[i] == '\n' {
            text.push('\n');
            i += 1;
            line += 1;
            line_start = i;
            continue;
        }
        if chars[i] != '$' {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'$') {
            text.push('$');
            i += 2;
            continue;
        }
        let close = (i + 1..chars.len())
            .find(|&j| chars[j] == '$')
            .ok_or_else(|| format!("unclosed `$` in a template near {:?}", tail(&chars, i)))?;
        let body: String = chars[i + 1..close].iter().collect();
        let before_on_line: String = chars[line_start..i].iter().collect();
        let ends_line = chars[close + 1..]
            .iter()
            .take_while(|c| **c != '\n')
            .all(|c| c.is_whitespace());
        found.push(Found {
            body,
            indent: before_on_line.clone(),
            before: std::mem::take(&mut text),
            line,
            starts_line: before_on_line.trim().is_empty(),
            ends_line,
        });
        i = close + 1;
    }
    // `text` is now whatever followed the last directive — or, when there
    // are none, the whole template with `$$` already collapsed. Returning
    // the raw template instead left every literal `$$` undoubled.
    Ok((found, text))
}

/// For each directive, whether the construct it belongs to spans more
/// than one line. A non-control directive is never trimmed, so `false`.
fn spans_lines(found: &[Found]) -> Result<Vec<bool>, String> {
    let mut multiline = vec![false; found.len()];
    // (index of the opener, its line, the indices of its `else`/`sep`)
    let mut open: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    for (index, directive) in found.iter().enumerate() {
        match keyword(directive.body.trim()) {
            "if" | "for" => open.push((index, directive.line, Vec::new())),
            "else" | "elseif" | "sep" => {
                open.last_mut()
                    .ok_or_else(|| format!("`${}$` with no `$if$` or `$for$`", directive.body))?
                    .2
                    .push(index);
            }
            "endif" | "endfor" => {
                let (opener, line, middles) = open
                    .pop()
                    .ok_or_else(|| format!("`${}$` with no `$if$` or `$for$`", directive.body))?;
                if directive.line != line {
                    multiline[opener] = true;
                    multiline[index] = true;
                    for middle in middles {
                        multiline[middle] = true;
                    }
                }
            }
            _ => {}
        }
    }
    if !open.is_empty() {
        return Err("a template `$if$` or `$for$` is never closed".to_owned());
    }
    Ok(multiline)
}


fn tail(chars: &[char], from: usize) -> String {
    chars[from..].iter().take(30).collect()
}

/// Build the tree. `until` is the directive that closes the current
/// block, so the caller can tell `$endif$` from `$else$`.
fn parse(tokens: &[Token], i: &mut usize, until: Option<&[&str]>) -> Result<Vec<Node>, String> {
    let mut nodes = Vec::new();
    while *i < tokens.len() {
        match &tokens[*i] {
            Token::Text(text) => {
                if !text.is_empty() {
                    nodes.push(Node::Text(text.clone()));
                }
                *i += 1;
            }
            Token::Directive { body, indent } => {
                let body = body.trim();
                if until.is_some_and(|ends| ends.contains(&keyword(body))) {
                    return Ok(nodes);
                }
                *i += 1;
                nodes.push(directive(body, indent, tokens, i)?);
            }
        }
    }
    if until.is_some() {
        return Err("a template `$if$` or `$for$` is never closed".to_owned());
    }
    Ok(nodes)
}

/// The word a directive starts with, for matching block ends.
fn keyword(body: &str) -> &str {
    match body.split_once('(') {
        Some((word, _)) => word,
        None => body,
    }
}

fn directive(
    body: &str,
    indent: &str,
    tokens: &[Token],
    i: &mut usize,
) -> Result<Node, String> {
    if let Some(name) = argument(body, "if") {
        let then = parse(tokens, i, Some(&["else", "endif"]))?;
        let closer = closing(tokens, i)?;
        let otherwise = if closer == "else" {
            let otherwise = parse(tokens, i, Some(&["endif"]))?;
            closing(tokens, i)?;
            otherwise
        } else {
            Vec::new()
        };
        return Ok(Node::If { name, then, otherwise });
    }
    if let Some(name) = argument(body, "for") {
        let body_nodes = parse(tokens, i, Some(&["sep", "endfor"]))?;
        let closer = closing(tokens, i)?;
        let separator = if closer == "sep" {
            let separator = parse(tokens, i, Some(&["endfor"]))?;
            closing(tokens, i)?;
            separator
        } else {
            Vec::new()
        };
        return Ok(Node::For { name: name.clone(), body: body_nodes, separator });
    }
    // `$styles.html()$` — a partial, which is any name ending in `()`.
    if let Some(name) = body.strip_suffix("()") {
        return Ok(Node::Partial { name: name.to_owned(), indent: indent.to_owned() });
    }
    // Everything pandoc's language has that this does not: say which.
    if body.contains('/') {
        return Err(format!(
            "template pipes are not supported: `${body}$`. This reads \
             `$var$`, `$if(var)$`, `$for(var)$`, `$sep$` and `$partial()$`"
        ));
    }
    if body.contains('(') || body.contains('.') && !body.ends_with("()") {
        return Err(format!(
            "`${body}$` is not a template construct this reads. Supported: \
             `$var$`, `$if(var)$ … $else$ … $endif$`, \
             `$for(var)$ … $sep$ … $endfor$`, `$partial()$`"
        ));
    }
    Ok(Node::Var { name: body.to_owned(), indent: indent.to_owned() })
}

/// Consume the directive that closed a block and return its keyword.
fn closing<'a>(tokens: &'a [Token], i: &mut usize) -> Result<&'a str, String> {
    match tokens.get(*i) {
        Some(Token::Directive { body, .. }) => {
            *i += 1;
            Ok(keyword(body.trim()))
        }
        _ => Err("a template `$if$` or `$for$` is never closed".to_owned()),
    }
}

/// `if(x)` -> `x`, for the named keyword only.
fn argument(body: &str, keyword: &str) -> Option<String> {
    body.strip_prefix(keyword)?
        .strip_prefix('(')?
        .strip_suffix(')')
        .map(str::to_owned)
}

fn emit(
    nodes: &[Node],
    context: &Context,
    partials: &dyn Fn(&str) -> Option<String>,
    out: &mut String,
) -> Result<(), String> {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Var { name, indent } => {
                let value = context.get(name).map_or("", Value::as_text);
                out.push_str(&indented(value, indent));
            }
            Node::If { name, then, otherwise } => {
                let taken = if context.get(name).is_some_and(Value::truthy) {
                    then
                } else {
                    otherwise
                };
                emit(taken, context, partials, out)?;
            }
            Node::For { name, body, separator } => {
                let items = match context.get(name) {
                    Some(Value::List(items)) => items.clone(),
                    // A non-list is one iteration over itself, which is
                    // how pandoc's `$for(author)$` works for one author.
                    Some(value) if value.truthy() => vec![value.as_text().to_owned()],
                    _ => Vec::new(),
                };
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        emit(separator, context, partials, out)?;
                    }
                    let mut scope = context.clone();
                    scope.insert(name.clone(), Value::Text(item.clone()));
                    emit(body, &scope, partials, out)?;
                }
            }
            Node::Partial { name, indent } => {
                let source = partials(name)
                    .ok_or_else(|| format!("no template partial named `{name}`"))?;
                let rendered = render(&source, context, partials)?;
                out.push_str(&indented(rendered.trim_end_matches('\n'), indent));
            }
        }
    }
    Ok(())
}

/// Every line after the first gets the same indentation as the first,
/// which is what pandoc does and what puts four spaces in front of all
/// 212 lines of the default stylesheet.
fn indented(value: &str, indent: &str) -> String {
    if indent.is_empty() || !value.contains('\n') {
        return value.to_owned();
    }
    value.replace('\n', &format!("\n{indent}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(pairs: &[(&str, Value)]) -> Context {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    fn out(template: &str, pairs: &[(&str, Value)]) -> String {
        render(template, &context(pairs), &|_| None).expect("rendered")
    }

    #[test]
    fn a_line_holding_only_a_directive_produces_no_line() {
        // Three lines of template, one line of output. Getting this
        // wrong puts a blank line into every conditional section of the
        // page, which is most of `<head>`.
        assert_eq!(out("$if(x)$\na\n$endif$\nb\n", &[("x", Value::Flag)]), "a\nb\n");
        assert_eq!(out("$if(x)$\na\n$endif$\nb\n", &[]), "b\n");
        // ...but a directive with text beside it keeps its line.
        assert_eq!(out("<i>$if(x)$y$endif$</i>\n", &[("x", Value::Flag)]), "<i>y</i>\n");
        // The two trims are independent, which the all-or-nothing rule
        // got wrong: `$if(x)$` at the end of a line eats its newline even
        // with text in front of it. Both cases are pandoc's, probed.
        assert_eq!(out("A$if(x)$\nB\n$endif$C\n", &[]), "AC\n");
        assert_eq!(out("A$if(x)$\nB\n$endif$C\n", &[("x", Value::Flag)]), "AB\nC\n");
    }

    #[test]
    fn an_indented_variable_indents_every_line_of_its_value() {
        // `    $styles.html()$` is how the default template includes 212
        // lines of CSS, and pandoc indents all of them.
        let value = Value::Text("a\nb\nc".to_owned());
        assert_eq!(out("  $x$\n", &[("x", value)]), "  a\n  b\n  c\n");
    }

    #[test]
    fn empty_is_false_but_still_interpolates() {
        // Pandoc's `lang` is set and empty in a document with no
        // language: `$if(dir)$` is false while `lang="$lang$"` writes
        // `lang=""`. Treating empty as absent drops the attribute.
        let empty = &[("lang", Value::Text(String::new()))];
        assert_eq!(out("lang=\"$lang$\"$if(lang)$!$endif$", empty), "lang=\"\"");
    }

    #[test]
    fn for_separates_and_else_is_taken() {
        let list = Value::List(vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(out("$for(k)$$k$$sep$, $endfor$", &[("k", list)]), "a, b");
        assert_eq!(out("$if(x)$y$else$n$endif$", &[]), "n");
        // An empty list is falsy and iterates nothing.
        assert_eq!(out("$for(k)$$k$$endfor$|", &[("k", Value::List(Vec::new()))]), "|");
    }

    #[test]
    fn a_partial_is_rendered_against_the_same_context() {
        let partials = |name: &str| (name == "p").then(|| "[$x$]".to_owned());
        let rendered = render("  $p()$\n", &context(&[("x", Value::Text("v".into()))]), &partials);
        assert_eq!(rendered.expect("rendered"), "  [v]\n");
    }

    #[test]
    fn what_is_not_supported_is_refused_by_name() {
        // A hole in a page is worse than a message: `$title/uppercase$`
        // silently producing nothing is a title that vanished.
        let piped = render("$title/uppercase$", &Context::new(), &|_| None);
        assert!(piped.expect_err("pipes").contains("pipes"), "pipes must be named");
        let unclosed = render("$if(x)$a", &Context::new(), &|_| None);
        assert!(unclosed.expect_err("unclosed").contains("closed"));
        let missing = render("$p()$", &Context::new(), &|_| None);
        assert!(missing.expect_err("partial").contains("partial"));
    }

    #[test]
    fn a_literal_dollar_is_doubled() {
        assert_eq!(out("cost: $$5\n", &[]), "cost: $5\n");
    }
}
