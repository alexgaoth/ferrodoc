//! Convert a markdown file to HTML in one process, for measurement.

fn main() {
    let md = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let html = ferrodoc_html::write_html(&ferrodoc_markdown::read_commonmark(&md));
    std::fs::write("/dev/null", html).unwrap();
}
