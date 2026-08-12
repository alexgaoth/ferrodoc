//! Probe: does comrak's own tree walk or drop overflow on deep nesting?

use comrak::nodes::AstNode;

fn depth<'a>(node: &'a AstNode<'a>) -> usize {
    1 + node.children().map(depth).max().unwrap_or(0)
}

fn main() {
    let md = std::fs::read_to_string("/tmp/deep.md").unwrap();
    {
        let arena = comrak::Arena::new();
        eprintln!("parse");
        let root = comrak::parse_document(&arena, &md, &comrak::Options::default());
        eprintln!("parsed; walking");
        eprintln!("depth = {}", depth(root));
        eprintln!("walked; dropping arena");
    }
    eprintln!("arena dropped");
}
