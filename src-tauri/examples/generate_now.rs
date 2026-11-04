//! Generates a document the same way the app's Generate button does (same checks, same cost ledger).
//! Usage: generate_now <db> <job-id> <resume|cover_letter> <learn: true|false>
use next_chapter_lib::{ai, generate, secrets};
use std::path::Path;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let db = Path::new(&a[0]);
    let docs = db.parent().expect("data dir").join("documents");
    let (prep, body) = {
        let c = next_chapter_lib::db::open(db).expect("db");
        let s = ai::get_settings(&c).expect("settings");
        assert!(s.enabled && s.acknowledged_at.is_some(), "AI writing is turned off in Preferences");
        let p = generate::prepare(&c, a[1].parse().unwrap(), &a[2], a[3] == "true").expect("prepare");
        let b = generate::request_body(&p);
        (p, b)
    };
    println!("model {} · effort {} · estimate ${:.2} · learning from previous: {}", prep.model, prep.effort, prep.estimated_usd, prep.includes_previous);
    let key = secrets::get("anthropic").expect("keychain").expect("no Anthropic key saved");
    let response = ai::send(&key, &body).await.unwrap_or_else(|e| panic!("{e}"));
    let c = next_chapter_lib::db::open(db).expect("db");
    let g = generate::finish(&c, &docs, &prep, &response).expect("finish");
    println!("saved document {} (v{}) · cost ${:.2} · findings {}", g.document.id, g.document.version, g.cost_usd, g.findings.len());
    for f in &g.findings { println!("  {:?}", f); }
    println!("notes: {}", g.notes_for_candidate);
}
