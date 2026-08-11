//! Write the same document twice and report whether the bytes match.

fn main() {
    let md = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let ast = ferrodoc_markdown::read_commonmark(&md);
    let a = ferrodoc_docx::write_docx(&ast).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let b = ferrodoc_docx::write_docx(&ast).unwrap();
    println!("ferrodoc bytes identical across runs: {} ({} bytes)", a == b, a.len());
}
