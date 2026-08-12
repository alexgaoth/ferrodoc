//! Split DOCX reading into unzip, XML parse and mapping, to show which
//! stage dominates.

use std::io::Read as _;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: docxprofile <file.docx>");
    let bytes = std::fs::read(&path).expect("readable file");
    let iters = 200;
    println!("{} ({} bytes), {iters} iterations:", path, bytes.len());

    let mut sink = 0usize;
    let start = Instant::now();
    for _ in 0..iters {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("zip");
        for name in ["word/document.xml", "word/numbering.xml", "word/styles.xml"] {
            if let Ok(mut file) = archive.by_name(name) {
                let mut s = String::new();
                file.read_to_string(&mut s).expect("utf8");
                sink += s.len();
            }
        }
    }
    println!("  unzip + read parts        {:>12?}", start.elapsed() / iters);

    let start = Instant::now();
    for _ in 0..iters {
        sink += ferrodoc_docx::read_docx(&bytes).expect("readable").blocks.len();
    }
    println!("  full read_docx            {:>12?}", start.elapsed() / iters);
    println!("  (sink {sink})");
}
