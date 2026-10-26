//! attract_scan <disc_root> [EVENT...] — scenes that name a `.wmv` and scenes reacting to the given
//! events (default `IDLE`), with the object and command. Used to find the front-end attract trailer.

use fh1_formats::zip::Archive;
use fh1_ui::anark::Scene;
use fh1_ui::hash::ahash31;
use fh1_ui::player::Player;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(a.get(1).map(String::as_str).unwrap_or("disc"));
    let events: Vec<String> = if a.len() > 2 { a[2..].to_vec() } else { vec!["IDLE".into()] };
    let hashes: Vec<(String, u32)> = events.iter().map(|e| (e.clone(), ahash31(e.as_bytes()))).collect();
    let mut ar = Archive::open(root.join("media/UI.zip")).expect("UI.zip");
    let entries: Vec<_> = ar.entries.iter().filter(|e| e.name.to_lowercase().ends_with(".bgf")).cloned().collect();
    for e in &entries {
        let Ok(bgf) = ar.read(e) else { continue };
        let Ok(scene) = Scene::load(&bgf, None, None) else { continue };
        let stem = e.name.rsplit(['/', '\\']).next().unwrap().to_string();
        let p = Player::new(scene);
        let wmvs: Vec<&String> = p.scene.bgf.strings.iter().filter(|s| s.to_lowercase().contains(".wmv")).collect();
        let mut hits = vec![];
        for (name, h) in &hashes {
            for handler in p.scene.bgf.handlers.iter() {
                for ev in handler.events.iter() {
                    if ev.hash31 & 0x7FFF_FFFF == *h {
                        let t = p.name(handler.object.max(0) as usize).unwrap_or("?").to_string();
                        let acts: Vec<String> = ev.actions.iter().map(|x| format!("{:?}", x.command)).collect();
                        hits.push(format!("{name} on {t} ({}): {}", ev.actions.len(), acts.join(",")));
                    }
                }
            }
        }
        if wmvs.is_empty() && hits.is_empty() {
            continue;
        }
        println!("== {stem} ==");
        for w in &wmvs {
            println!("   movie {w}");
        }
        for h in &hits {
            println!("   {h}");
        }
    }
}
