//! Saves an edited document as a new version, the same way the app's editor does (claim check, Word and PDF).
//! Usage: save_edit_now <db> <document-id> <content.json>
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let db = Path::new(&a[0]);
    let docs = db.parent().expect("data dir").join("documents");
    let content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[2]).expect("read")).expect("json");
    let c = next_chapter_lib::db::open(db).expect("db");
    let g = next_chapter_lib::generate::save_edit(&c, &docs, a[1].parse().unwrap(), &content).expect("save");
    println!("saved document {} (v{}) · findings {}", g.document.id, g.document.version, g.findings.len());
    for f in &g.findings { println!("  {:?}", f); }
    if let Some(p) = g.document.pdf_path { println!("pdf: {p}"); }
}
