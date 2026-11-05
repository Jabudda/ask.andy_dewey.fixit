//! Prints the review queue and one job's application packet (writes proposals into the given database).
//! Usage: packet_preview <db> <job-id>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let c = next_chapter_lib::db::open(std::path::Path::new(&a[0])).expect("db");
    for q in next_chapter_lib::application::queue(&c).expect("queue") {
        println!("[{}] #{} {} · resume ok {} · blockers {:?}", q.job.status, q.job.id, q.job.draft.title, q.resume_approved, q.blockers);
    }
    let p = next_chapter_lib::application::packet(&c, a[1].parse().unwrap()).expect("packet");
    println!("\nblockers: {:?}", p.blockers);
    for x in p.answers { println!("- {} [{}] {:?} ({})", x.question, x.status, x.answer, x.source); }
}
