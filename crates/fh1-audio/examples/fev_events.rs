//! `fev_events <file.fev>`: list a FEV's events with their wave references.
fn main() {
    let p = std::env::args().nth(1).unwrap();
    match fh1_audio::fev::Fev::load(std::path::Path::new(&p)) {
        Ok(f) => {
            for e in &f.events {
                let mut waves: Vec<String> = e.waves.iter().map(|w| w.name.clone()).collect();
                waves.dedup();
                println!("{} : {}", e.full_path(), waves.join(", "));
            }
        }
        Err(e) => println!("parse error: {e:#}"),
    }
}
