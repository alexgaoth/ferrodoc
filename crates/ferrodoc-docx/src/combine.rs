//! A port of pandoc's `Text.Pandoc.Readers.Docx.Combine`.
//!
//! OOXML formatting does not nest: each run carries its full set of
//! properties. This module reconstructs nesting the way pandoc does —
//! adjacent sequences are combined by extracting the stack of formatting
//! modifiers on each side of the boundary and merging under the shared
//! part, spacing boundary whitespace out of formatting containers.

use ferrodoc_ast::{Attr, Block, Inline, Target};

/// A formatting wrapper extracted from (or applied to) inline content.
///
/// Mirrors pandoc's `Modifier Inlines`: equality is by constructor and
/// payload, and `Span` is the only "attribute modifier" (kept even around
/// empty content).
#[derive(Clone, PartialEq)]
pub enum Modifier {
    Emph,
    Strong,
    SmallCaps,
    Strikeout,
    Underline,
    Superscript,
    Subscript,
    // Boxed to match `Inline`, so unstacking and restacking a
    // modifier moves the box rather than cloning what it holds.
    Link(Box<Attr>, Box<Target>),
    Span(Box<Attr>),
}

impl Modifier {
    fn is_attr(&self) -> bool {
        matches!(self, Modifier::Span(_))
    }

    fn apply(&self, contents: Vec<Inline>) -> Inline {
        match self {
            Modifier::Emph => Inline::Emph(contents),
            Modifier::Strong => Inline::Strong(contents),
            Modifier::SmallCaps => Inline::SmallCaps(contents),
            Modifier::Strikeout => Inline::Strikeout(contents),
            Modifier::Underline => Inline::Underline(contents),
            Modifier::Superscript => Inline::Superscript(contents),
            Modifier::Subscript => Inline::Subscript(contents),
            Modifier::Link(attr, target) => {
                Inline::Link(attr.clone(), contents, target.clone())
            }
            Modifier::Span(attr) => Inline::Span(attr.clone(), contents),
        }
    }
}

/// Apply modifiers outermost-first, skipping plain modifiers around empty
/// content (attribute modifiers are kept, like pandoc).
pub fn stack(modifiers: &[Modifier], contents: Vec<Inline>) -> Vec<Inline> {
    match modifiers.split_first() {
        None => contents,
        Some((m, rest)) => {
            let inner = stack(rest, contents);
            if inner.is_empty() && !m.is_attr() {
                inner
            } else {
                vec![m.apply(inner)]
            }
        }
    }
}

/// Decompose a singleton chain of formatting containers into its modifier
/// stack (outermost first) and innermost contents.
fn unstack(mut inlines: Vec<Inline>) -> (Vec<Modifier>, Vec<Inline>) {
    let mut modifiers = Vec::new();
    loop {
        if inlines.len() != 1 {
            return (modifiers, inlines);
        }
        let (modifier, inner) = match inlines.remove(0) {
            Inline::Emph(l) => (Modifier::Emph, l),
            Inline::Strong(l) => (Modifier::Strong, l),
            Inline::SmallCaps(l) => (Modifier::SmallCaps, l),
            Inline::Strikeout(l) => (Modifier::Strikeout, l),
            Inline::Underline(l) => (Modifier::Underline, l),
            Inline::Superscript(l) => (Modifier::Superscript, l),
            Inline::Subscript(l) => (Modifier::Subscript, l),
            Inline::Link(attr, l, target) => (Modifier::Link(attr, target), l),
            Inline::Span(attr, l) => (Modifier::Span(attr), l),
            other => return (modifiers, vec![other]),
        };
        modifiers.push(modifier);
        inlines = inner;
    }
}

fn is_space(inline: &Inline) -> bool {
    matches!(inline, Inline::Space | Inline::SoftBreak)
}

/// Leading spaces, the modifier stack and core content, trailing spaces.
type SpacedOut = (Vec<Inline>, (Vec<Modifier>, Vec<Inline>), Vec<Inline>);

/// Split into (leading spaces, (modifier stack, core), trailing spaces).
fn space_out(inlines: Vec<Inline>) -> SpacedOut {
    let (modifiers, mut inner) = unstack(inlines);
    let mut left = Vec::new();
    while inner.first().is_some_and(is_space) {
        left.push(inner.remove(0));
    }
    let mut right = Vec::new();
    while inner.last().is_some_and(is_space) {
        right.insert(0, inner.pop().expect("checked non-empty"));
    }
    (left, (modifiers, inner), right)
}

/// Hoist leading spaces out of the formatting stack.
fn space_out_l(inlines: Vec<Inline>) -> (Vec<Inline>, Vec<Inline>) {
    let (left, (modifiers, core), right) = space_out(inlines);
    (left, stack(&modifiers, append(core, right)))
}

/// Hoist trailing spaces out of the formatting stack.
fn space_out_r(inlines: Vec<Inline>) -> (Vec<Inline>, Vec<Inline>) {
    let (left, (modifiers, core), right) = space_out(inlines);
    (stack(&modifiers, append(left, core)), right)
}

/// Multiset intersection preserving the left operand's order.
fn intersect(a: &[Modifier], b: &[Modifier]) -> Vec<Modifier> {
    let mut pool: Vec<&Modifier> = b.iter().collect();
    let mut shared = Vec::new();
    for m in a {
        if let Some(i) = pool.iter().position(|x| *x == m) {
            pool.remove(i);
            shared.push(m.clone());
        }
    }
    shared
}

/// Multiset difference: `a` minus one occurrence of each element of `b`.
fn difference(a: &[Modifier], b: &[Modifier]) -> Vec<Modifier> {
    let mut pool: Vec<&Modifier> = b.iter().collect();
    let mut rest = Vec::new();
    for m in a {
        if let Some(i) = pool.iter().position(|x| *x == m) {
            pool.remove(i);
        } else {
            rest.push(m.clone());
        }
    }
    rest
}

/// Append with pandoc `Inlines` builder semantics: at the seam, adjacent
/// `Str`s and adjacent same-type formatting merge, and whitespace collapses
/// with a hard break winning over a soft one.
pub fn append(mut a: Vec<Inline>, b: Vec<Inline>) -> Vec<Inline> {
    for inline in b {
        match (a.last_mut(), inline) {
            (Some(Inline::Str(prev)), Inline::Str(next)) => prev.push_str(&next),
            (Some(Inline::Emph(prev)), Inline::Emph(next))
            | (Some(Inline::Strong(prev)), Inline::Strong(next))
            | (Some(Inline::Strikeout(prev)), Inline::Strikeout(next))
            | (Some(Inline::Superscript(prev)), Inline::Superscript(next))
            | (Some(Inline::Subscript(prev)), Inline::Subscript(next)) => {
                let merged = append(std::mem::take(prev), next);
                *prev = merged;
            }
            // Space collapses into whatever break it meets.
            (Some(Inline::Space), Inline::Space)
            | (
                Some(Inline::SoftBreak | Inline::LineBreak),
                Inline::Space | Inline::SoftBreak,
            ) => {}
            (Some(last @ Inline::Space), next @ (Inline::SoftBreak | Inline::LineBreak))
            | (Some(last @ Inline::SoftBreak), next @ Inline::LineBreak) => *last = next,
            (_, inline) => a.push(inline),
        }
    }
    a
}

/// Combine two inline sequences at their boundary, merging the boundary
/// elements under any shared formatting stack.
pub fn combine_inlines(mut x: Vec<Inline>, mut y: Vec<Inline>) -> Vec<Inline> {
    let boundary_x = if x.is_empty() { Vec::new() } else { vec![x.pop().expect("non-empty")] };
    let boundary_y = if y.is_empty() { Vec::new() } else { vec![y.remove(0)] };
    append(append(x, combine_singletons(boundary_x, boundary_y)), y)
}

fn combine_singletons(x: Vec<Inline>, y: Vec<Inline>) -> Vec<Inline> {
    let (xfs, xs) = unstack(x);
    let (yfs, ys) = unstack(y);
    let shared = intersect(&xfs, &yfs);
    let x_remaining = difference(&xfs, &shared);
    let y_remaining = difference(&yfs, &shared);

    if shared.is_empty() {
        let x_rem_attr: Vec<Modifier> =
            x_remaining.iter().filter(|m| m.is_attr()).cloned().collect();
        let y_rem_attr: Vec<Modifier> =
            y_remaining.iter().filter(|m| m.is_attr()).cloned().collect();
        return match (xs.is_empty(), ys.is_empty()) {
            (true, true) => {
                append(stack(&x_rem_attr, Vec::new()), stack(&y_rem_attr, Vec::new()))
            }
            (true, false) => {
                let (sp, y2) = space_out_l(stack(&yfs, ys));
                append(append(stack(&x_rem_attr, Vec::new()), sp), y2)
            }
            (false, true) => {
                let (x2, sp) = space_out_r(stack(&xfs, xs));
                append(append(x2, sp), stack(&y_rem_attr, Vec::new()))
            }
            (false, false) => {
                let (x2, xsp) = space_out_r(stack(&xfs, xs));
                let (ysp, y2) = space_out_l(stack(&yfs, ys));
                append(append(append(x2, xsp), ysp), y2)
            }
        };
    }
    stack(
        &shared,
        combine_inlines(stack(&x_remaining, xs), stack(&y_remaining, ys)),
    )
}

/// Fold [`combine_inlines`] over sequences, with pandoc's final
/// combine-with-empty pass that spaces out the trailing boundary.
pub fn smush_inlines(sequences: Vec<Vec<Inline>>) -> Vec<Inline> {
    let folded = sequences
        .into_iter()
        .fold(Vec::new(), combine_inlines);
    combine_inlines(folded, Vec::new())
}

/// Combine two block sequences: adjacent block quotes merge, and adjacent
/// code blocks with equal attributes merge with a newline.
pub fn combine_blocks(mut x: Vec<Block>, mut y: Vec<Block>) -> Vec<Block> {
    if let (Some(Block::BlockQuote(a)), Some(Block::BlockQuote(_))) = (x.last_mut(), y.first()) {
        let Some(Block::BlockQuote(b)) = y.first_mut() else { unreachable!("just matched") };
        a.append(b);
        y.remove(0);
    } else if let (Some(Block::CodeBlock(attr_a, a)), Some(Block::CodeBlock(attr_b, _))) =
        (x.last_mut(), y.first())
        && attr_a == attr_b
    {
        let Some(Block::CodeBlock(_, b)) = y.first_mut() else { unreachable!("just matched") };
        a.push('\n');
        a.push_str(b);
        y.remove(0);
    }
    x.extend(y);
    x
}

/// Fold [`combine_blocks`] over sequences.
pub fn smush_blocks(sequences: Vec<Vec<Block>>) -> Vec<Block> {
    sequences.into_iter().fold(Vec::new(), combine_blocks)
}
