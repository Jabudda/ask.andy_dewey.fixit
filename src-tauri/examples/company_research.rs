//! cargo run --example company_research -- <db> <job-id> <website> [--save]
//! Reads the employer's website for a job and prints what was found; --save stores it (keeping earlier choices).
use next_chapter_lib::{company, db};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = db::open(std::path::Path::new(&a[1])).expect("open db");
    let job = db::get_job(&c, a[2].parse().unwrap()).expect("job");
    let mut r = company::read_site(&a[3], &job.draft.employer, &job.draft.title).await.expect("read");
    for p in &r.pages { println!("page  {} · {} · {} chars", p.url, p.title, p.chars); }
    for s in &r.skipped { println!("skip  {s}"); }
    for t in &r.terms { println!("{} {:8} {}", if t.selected { "[x]" } else { "[ ]" }, t.kind, t.term); }
    if a.iter().any(|x| x == "--save") {
        if let Some(old) = company::get(&c, &job.draft.employer).unwrap() { r.terms = company::merge_previous(r.terms, &old.terms); }
        company::save(&c, &r).unwrap();
        println!("saved for {}", r.employer);
    }
    println!("\n{}", company::prompt_block(&r));
}
