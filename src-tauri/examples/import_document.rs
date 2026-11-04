//! Imports one file into an app database as a profile document.
//! Usage: import_document <db> <documents-dir> <file> <kind>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = next_chapter_lib::db::open(std::path::Path::new(&a[0])).expect("db");
    let r = next_chapter_lib::commands::import_document_into(&c, std::path::Path::new(&a[1]), std::path::Path::new(&a[2]), &a[3]).expect("import");
    println!("imported {} v{} ({} characters of text)", r.document.file_name, r.document.version, r.document.extracted_text.len());
}
