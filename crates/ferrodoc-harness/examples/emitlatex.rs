//! Emit our LaTeX for one markdown file, to compare against pandoc's.

fn main() {
    let md = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let ast = ferrodoc_markdown::read_gfm(&md).unwrap();
    print!("{}", ferrodoc_latex::write_latex(&ast));
}
