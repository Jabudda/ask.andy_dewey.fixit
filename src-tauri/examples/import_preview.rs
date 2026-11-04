//! Dry-run preview of a folder import against an empty in-memory database.
//! Usage: cargo run --example import_preview -- "/path/to/Command Center"
fn main() {
    let root = std::env::args().nth(1).expect("folder path");
    let c = next_chapter_lib::db::open_in_memory().expect("db");
    let tmp = std::env::temp_dir().join("nc-preview-docs");
    let r = next_chapter_lib::command_center::run(&c, &tmp, std::path::Path::new(&root), true).expect("import");
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
}
