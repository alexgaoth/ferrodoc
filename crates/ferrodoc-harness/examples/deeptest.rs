//! Probe: which stage overflows on deeply nested markdown?

fn main() {
    let md = std::fs::read_to_string("/tmp/deep.md").unwrap();
    eprintln!("stage 1: parse");
    let doc = ferrodoc_markdown::read_commonmark(&md).unwrap();
    eprintln!("stage 1 ok, {} blocks", doc.blocks.len());
    eprintln!("stage 2: write html");
    let html = ferrodoc_html::write_html(&doc);
    eprintln!("stage 2 ok, {} bytes", html.len());
    eprintln!("stage 3: drop");
    drop(doc);
    eprintln!("stage 3 ok");
}
