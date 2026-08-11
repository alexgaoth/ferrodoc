//! Golden round-trip tests against real pandoc JSON output, plus a
//! coverage test proving the fixtures exercise every AST constructor.

use ferrodoc_ast::Pandoc;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures directory exists")
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no JSON fixtures found in {}; run tests/fixtures/generate.sh",
        dir.display()
    );
    files
}

#[test]
fn every_fixture_round_trips_losslessly() {
    for path in fixture_files() {
        let raw = std::fs::read_to_string(&path).expect("fixture is readable");
        let original: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
        let doc: Pandoc = serde_json::from_value(original.clone())
            .unwrap_or_else(|e| panic!("{} failed to deserialize: {e}", path.display()));
        let back = serde_json::to_value(&doc).expect("serialization cannot fail");
        assert_eq!(
            original,
            back,
            "{} did not round-trip to an equal JSON value",
            path.display()
        );
    }
}

/// Every `"t"` tag that must appear somewhere across the fixtures.
const REQUIRED_TAGS: &[&str] = &[
    // Block (14)
    "Plain", "Para", "LineBlock", "CodeBlock", "RawBlock", "BlockQuote", "OrderedList",
    "BulletList", "DefinitionList", "Header", "HorizontalRule", "Table", "Figure", "Div",
    // Inline (20)
    "Str", "Emph", "Underline", "Strong", "Strikeout", "Superscript", "Subscript",
    "SmallCaps", "Quoted", "Cite", "Code", "Space", "SoftBreak", "LineBreak", "Math",
    "RawInline", "Link", "Image", "Note", "Span",
    // MetaValue (6)
    "MetaMap", "MetaList", "MetaBool", "MetaString", "MetaInlines", "MetaBlocks",
    // Alignment (4)
    "AlignLeft", "AlignRight", "AlignCenter", "AlignDefault",
    // ColWidth (2)
    "ColWidth", "ColWidthDefault",
    // QuoteType (2)
    "SingleQuote", "DoubleQuote",
    // MathType (2)
    "DisplayMath", "InlineMath",
    // CitationMode (3)
    "AuthorInText", "SuppressAuthor", "NormalCitation",
    // ListNumberStyle (7)
    "DefaultStyle", "Example", "Decimal", "LowerRoman", "UpperRoman", "LowerAlpha",
    "UpperAlpha",
    // ListNumberDelim (4)
    "DefaultDelim", "Period", "OneParen", "TwoParens",
];

fn collect_tags(value: &Value, tags: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(t)) = map.get("t") {
                tags.insert(t.clone());
            }
            for v in map.values() {
                collect_tags(v, tags);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_tags(v, tags);
            }
        }
        _ => {}
    }
}

#[test]
fn fixtures_cover_every_constructor() {
    let mut seen = BTreeSet::new();
    for path in fixture_files() {
        let raw = std::fs::read_to_string(&path).expect("fixture is readable");
        let original: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
        collect_tags(&original, &mut seen);
    }
    let missing: Vec<&&str> = REQUIRED_TAGS
        .iter()
        .filter(|t| !seen.contains(**t))
        .collect();
    assert!(
        missing.is_empty(),
        "fixtures never exercise these constructors: {missing:?}"
    );
}
