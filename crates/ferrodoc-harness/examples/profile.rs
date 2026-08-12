//! Time each conversion stage separately, to show where the work is.

use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: profile <file.md>");
    let md = std::fs::read_to_string(&path).expect("readable file");
    let iters = 300;

    let mut sink = 0usize;
    let time = |name: &str, mut f: Box<dyn FnMut() -> usize>| {
        f();
        let start = Instant::now();
        let mut acc = 0usize;
        for _ in 0..iters {
            acc += f();
        }
        let each = start.elapsed() / iters;
        println!("  {name:<28} {each:>12?}");
        acc
    };

    println!("{} ({} bytes), {iters} iterations each:", path, md.len());
    let md0 = md.clone();
    sink += time("  of which: comrak parse", Box::new(move || {
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, &md0, &comrak::Options::default());
        root.children().count()
    }));
    let md1 = md.clone();
    sink += time("parse markdown", Box::new(move || {
        ferrodoc_markdown::read_commonmark(&md1).unwrap().blocks.len()
    }));
    let ast = ferrodoc_markdown::read_commonmark(&md).unwrap();
    let a1 = ast.clone();
    sink += time("write html", Box::new(move || ferrodoc_html::write_html(&a1).len()));
    let a2 = ast.clone();
    sink += time("write docx", Box::new(move || {
        ferrodoc_docx::write_docx(&a2).expect("writable").len()
    }));
    let docx = ferrodoc_docx::write_docx(&ast).expect("writable");
    sink += time("read docx", Box::new(move || {
        ferrodoc_docx::read_docx(&docx).expect("readable").blocks.len()
    }));
    println!("  (sink {sink})");
}
