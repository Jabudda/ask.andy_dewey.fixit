//! Live discovery against a throwaway database. Usage:
//! cargo run --example discover_preview -- <db-path> greenhouse:smartsheet ashby:vetcove lever:civitech ...
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let db_path = std::path::PathBuf::from(args.next().expect("db path"));
    let c = next_chapter_lib::db::open(&db_path).expect("db");
    for spec in args {
        let (kind, id) = spec.split_once(':').expect("kind:identifier");
        next_chapter_lib::discovery::save_source(&c, &next_chapter_lib::models::JobSource {
            kind: kind.into(), identifier: id.into(), enabled: true, ..Default::default()
        }).expect("source");
    }
    drop(c);
    let runs = next_chapter_lib::discovery::run_with_path(&db_path, None, "manual").await.expect("run");
    for r in &runs {
        println!("{:<12} found {:>4} new {:>3} filtered {:>4} excluded {:>3} dup {:>2} {}{}",
            r.label, r.found, r.new_jobs, r.filtered, r.excluded, r.duplicates,
            r.error.clone().unwrap_or_default(), r.skipped_reason.clone().unwrap_or_default());
    }
    let c = next_chapter_lib::db::open(&db_path).unwrap();
    let mut jobs = next_chapter_lib::db::list_jobs(&c).unwrap();
    jobs.sort_by_key(|j| -j.score.unwrap_or(0));
    for j in jobs.iter().take(15) {
        println!("{:>3} | {:<28} | {:<45} | {:<7} | {:<9} | {:?}-{:?}/{:?} | {}",
            j.score.unwrap_or(-1), &j.draft.employer.chars().take(28).collect::<String>(), &j.draft.title.chars().take(45).collect::<String>(),
            j.draft.remote_type, j.draft.wisconsin_eligibility, j.draft.salary_min, j.draft.salary_max, j.draft.salary_period,
            j.score_breakdown.as_ref().map(|b| b.recommendation.clone()).unwrap_or_default());
    }
}
