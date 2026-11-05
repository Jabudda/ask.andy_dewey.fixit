//! recheck <db> <doc-id>: runs the claim check on a draft against the current profile and prints it.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = next_chapter_lib::db::open(std::path::Path::new(&a[1])).expect("db");
    let before = next_chapter_lib::db::get_document(&c, a[2].parse().unwrap()).unwrap().claim_report;
    println!("before: {} findings", before.len());
    for f in &before { println!("  {} | {} | {}", f.severity, f.location, f.message); }
    let after = next_chapter_lib::generate::recheck(&c, a[2].parse().unwrap()).unwrap();
    println!("after: {} findings", after.len());
    for f in &after { println!("  {} | {} | {}", f.severity, f.location, f.message); }
}
