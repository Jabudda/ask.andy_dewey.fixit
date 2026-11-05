//! rerender <db> <doc-id> <out.pdf>: lays a stored resume/cover letter out again with the current
//! PDF renderer (content unchanged) and prints which groups were moved to keep roles whole.
use next_chapter_lib::{db, docgen, tailoring};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = db::open(std::path::Path::new(&a[1])).expect("db");
    let d = db::get_document(&c, a[2].parse().unwrap()).expect("doc");
    let p = db::get_profile(&c).unwrap();
    let content = d.content_json.clone().expect("generated document");
    let blocks = if d.kind == "generated_resume" {
        tailoring::render_resume(&p, &serde_json::from_value(content).unwrap())
    } else {
        let job = db::get_job(&c, d.job_id.unwrap()).unwrap();
        tailoring::render_cover_letter(&p, &job, &serde_json::from_value(content).unwrap(), "November 4, 2026")
    };
    let (bytes, moved) = docgen::layout_pdf(&blocks, "Resume").unwrap();
    std::fs::write(&a[3], bytes).unwrap();
    let groups = docgen::keep_together_groups(&blocks);
    for g in moved { println!("moved to next page: {:?}", &blocks[groups[g].start]); }
}
