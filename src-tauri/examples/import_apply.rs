//! Runs a real (non-dry-run) folder import into an app database, then re-scores.
//! Usage: cargo run --example import_apply -- <db-path> <documents-dir> <folder>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = next_chapter_lib::db::open(std::path::Path::new(&a[0])).expect("db");
    let r = next_chapter_lib::command_center::run(&c, std::path::Path::new(&a[1]), std::path::Path::new(&a[2]), false).expect("import");
    next_chapter_lib::db::score_jobs(&c, None).unwrap();
    println!("jobs added {} · documents added {} · certifications {} · warnings {}",
        r.jobs.iter().filter(|j| j.action == "added").count(),
        r.documents.iter().filter(|d| d.action != "already imported").count(),
        r.certifications.len(), r.warnings.len());
    println!("profile: {:?}", r.profile_updates);
}
