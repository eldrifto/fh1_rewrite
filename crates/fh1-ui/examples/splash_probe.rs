//! splash_probe <scene_dir> <SCENE> [name-filter] — lists every component (duration, slides with
//! start/end ms) and, for objects whose name contains the filter (all when omitted), their decoded
//! properties. Used while matching the 120_P_PRE_SPLASH title screen.

use fh1_ui::anark::bgf::ObjectKind;
use fh1_ui::anark::Scene;
use fh1_ui::player::Player;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let base = std::path::Path::new(&a[1]).join(a[2].to_lowercase());
    let filter = a.get(3).map(|s| s.to_lowercase());
    let read = |e: &str| std::fs::read(base.with_extension(e)).ok();
    let p = Player::new(Scene::load(&read("bgf").unwrap(), read("fbf").as_deref(), read("bsg").as_deref()).unwrap());
    let b = &p.scene.bgf;
    if filter.as_deref() == Some("strings") {
        for (i, s) in b.strings.iter().enumerate() {
            println!("STR {i}: {s}");
        }
        return;
    }
    let name = |i: i32| p.name(usize::try_from(i).unwrap_or(usize::MAX)).unwrap_or("?").to_string();
    println!("== components ==");
    for (i, o) in b.objects.iter().enumerate() {
        if o.kind != ObjectKind::Component {
            continue;
        }
        let ci = o.component.unwrap();
        println!("component {i} {}  slides {}  duration {} ms", name(i as i32), ci.n_slides, ci.duration);
        for s in b.slides.iter().filter(|s| s.component as usize == i) {
            let refs: Vec<String> = s
                .entries
                .iter()
                .flat_map(|e| e.refs.iter())
                .filter_map(|r| r.track())
                .map(|t| b.tracks.get(t).map(|t| fh1_ui::names::prop_name(t.key).unwrap_or("?").to_string()).unwrap_or_default())
                .collect();
            println!("    slide {:<14} flags {:>3}  {}..{} ms  entries {}  tracks[{}]", s.name, s.flags, s.start, s.end, s.entries.len(), refs.join(","));
        }
    }
    println!("== props ==");
    for (i, o) in b.objects.iter().enumerate() {
        let n = name(i as i32);
        if let Some(f) = &filter {
            if !n.to_lowercase().contains(f) {
                continue;
            }
        }
        for pr in &o.props {
            println!("{i:>4} {n}.{} = {:?}", fh1_ui::names::prop_name(pr.key).unwrap_or("?"), pr.value());
        }
        for s in b.slides.iter().filter(|s| s.entries.iter().any(|e| e.object == i as i32)) {
            for e in s.entries.iter().filter(|e| e.object == i as i32) {
                for pr in &e.props {
                    println!("{i:>4} [slide {}] {n}.{} = {:?}", s.name, fh1_ui::names::prop_name(pr.key).unwrap_or("?"), pr.value());
                }
            }
        }
        for t in b.tracks.iter().filter(|t| t.object == i as i32) {
            let keys: Vec<String> = t.keys.iter().map(|k| format!("{}ms={}", k.time, k.value)).collect();
            println!("{i:>4} {n} TRACK {}: {}", fh1_ui::names::prop_name(t.key).unwrap_or("?"), keys.join(" "));
        }
    }
}
