//! Re-checks and re-scores every job; optionally corrects one job's title and employer first.
//! Usage: retitle_and_rescore <db> [<job-id> <title> <employer>]
fn main() {
    use next_chapter_lib::db;
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = db::open(std::path::Path::new(&a[0])).expect("db");
    if a.len() >= 4 {
        let id: i64 = a[1].parse().unwrap();
        let mut d = db::get_job(&c, id).unwrap().draft;
        d.title = a[2].clone();
        d.employer = a[3].clone();
        d.detection_notes.retain(|n| !n.contains("Employer not detected"));
        d.detection_notes.push("Title and employer corrected from the page title".into());
        db::update_job(&c, id, &d).unwrap();
    }
    let p = db::get_preferences(&c).unwrap();
    println!("re-checked and re-scored {} jobs", db::reevaluate_all(&c, &p).unwrap());
}
