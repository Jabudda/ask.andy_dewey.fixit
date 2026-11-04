//! Loads a verified career profile from a JSON file into an app database.
//! The JSON holds personal data, so keep it outside the repository.
//! Usage: cargo run --example load_profile -- <db-path> <profile.json>
use next_chapter_lib::{db, models::*};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileFile {
    basics: ProfileBasics,
    #[serde(default)] experience: Vec<Experience>,
    #[serde(default)] education: Vec<Education>,
    #[serde(default)] skills: Vec<Skill>,
    #[serde(default)] certifications: Vec<Certification>,
    #[serde(default)] accomplishments: Vec<Accomplishment>,
}

fn main() {
    let mut a = std::env::args().skip(1);
    let db_path = a.next().expect("db path");
    let file = a.next().expect("profile json");
    let p: ProfileFile = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).expect("valid profile JSON");
    let c = db::open(std::path::Path::new(&db_path)).expect("db");
    let existing = db::get_profile(&c).unwrap();
    db::save_profile_basics(&c, &p.basics).unwrap();
    let mut n = (0, 0, 0, 0, 0);
    for (i, mut e) in p.experience.into_iter().enumerate() {
        if existing.experience.iter().any(|x| x.employer == e.employer && x.title == e.title) { continue; }
        e.sort_order = i as i64;
        db::save_experience(&c, &e).unwrap();
        n.0 += 1;
    }
    for e in p.education {
        if existing.education.iter().any(|x| x.institution == e.institution) { continue; }
        db::save_education(&c, &e).unwrap();
        n.1 += 1;
    }
    for s in p.skills { db::save_skill(&c, &s).unwrap(); n.2 += 1; }
    for x in p.certifications {
        if existing.certifications.iter().any(|y| y.name.eq_ignore_ascii_case(&x.name)) { continue; }
        db::save_certification(&c, &x).unwrap();
        n.3 += 1;
    }
    for x in p.accomplishments { db::save_accomplishment(&c, &x).unwrap(); n.4 += 1; }
    let scored = db::score_jobs(&c, None).unwrap();
    db::log(&c, "info", "profile", &format!("Profile loaded from file: {} positions, {} education, {} skills, {} certifications, {} accomplishments", n.0, n.1, n.2, n.3, n.4));
    println!("positions {} · education {} · skills {} · certifications {} · accomplishments {} · rescored {scored} jobs", n.0, n.1, n.2, n.3, n.4);
}
