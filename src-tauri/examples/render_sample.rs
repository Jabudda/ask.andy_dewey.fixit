//! Renders a resume from a profile database WITHOUT calling Claude: uses each position's
//! existing bullets verbatim. For checking layout. Usage: render_sample <db> <out-dir>
fn main() {
    use next_chapter_lib::{db, docgen, tailoring::*};
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = db::open(std::path::Path::new(&a[0])).unwrap(); // pass a COPY of the database
    let p = db::get_profile(&c).unwrap();
    let pos = positions(&p);
    let r = ResumeContent {
        headline: p.basics.headline.clone(),
        summary: Line { text: p.basics.summary.clone(), sources: vec!["S1".into()] },
        skills: p.skills.iter().take(12).map(|s| s.name.clone()).collect(),
        positions: pos.iter().enumerate().map(|(i, x)| PositionContent {
            position_id: x.id.clone(),
            include: i < 3,
            bullets: x.exp.achievements.lines().filter(|l| !l.trim().is_empty()).take(if i == 0 { 5 } else { 2 })
                .enumerate().map(|(j, l)| Line { text: l.trim().into(), sources: vec![format!("E{i}.{j}")] }).collect(),
        }).collect(),
        notes_for_candidate: String::new(),
    };
    let blocks = render_resume(&p, &r);
    let out = std::path::Path::new(&a[1]);
    docgen::write_docx(&blocks, &out.join("sample_resume.docx")).unwrap();
    docgen::write_pdf(&blocks, "Sample resume", &out.join("sample_resume.pdf")).unwrap();
    println!("ok");
}
