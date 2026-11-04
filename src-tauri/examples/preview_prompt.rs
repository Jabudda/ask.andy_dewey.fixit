//! Shows what a generation would send, without calling Claude.
//! Usage: preview_prompt <db> <job-id> <resume|cover_letter> <learn: true|false>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = next_chapter_lib::db::open(std::path::Path::new(&a[0])).expect("db");
    let p = next_chapter_lib::generate::prepare(&c, a[1].parse().unwrap(), &a[2], a[3] == "true").expect("prepare");
    println!("model {} · effort {} · facts {} · estimate ${:.2} · previous {:?} · included {}", p.model, p.effort, p.fact_count, p.estimated_usd, p.previous_available, p.includes_previous);
    println!("prompt characters: system {} · user {}", p.system.len(), p.user.len());
    if let Some(i) = p.user.find("<previous_application") { println!("--- lesson excerpt ---\n{}", &p.user[i..(i + 600).min(p.user.len())]); }
}
