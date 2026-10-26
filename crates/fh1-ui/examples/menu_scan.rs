//! menu_scan <scene_dir> [name-filter] — scenes (a directory of .bgf with sibling .bsg, e.g. all of
//! UI.zip's Scenes/ui4 extracted) that contain an object whose name contains the filter (default
//! "button"), printed with those names. Used to find the front-end menu scenes.

use fh1_ui::anark::Scene;
use fh1_ui::player::Player;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&a[1]);
    let filter = a.get(2).map(|s| s.to_lowercase()).unwrap_or_else(|| "button".into());
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bgf")).collect();
    files.sort();
    for p in files {
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let read = |e: &str| std::fs::read(p.with_extension(e)).ok();
        let Ok(scene) = Scene::load(&read("bgf").unwrap(), read("fbf").as_deref(), read("bsg").as_deref()) else { continue };
        let pl = Player::new(scene);
        let hits: Vec<String> = (0..pl.scene.bgf.objects.len())
            .filter_map(|i| pl.name(i).map(|n| (i, n)))
            .filter(|(_, n)| n.to_lowercase().contains(&filter))
            .map(|(i, n)| format!("{i}:{n}"))
            .collect();
        if !hits.is_empty() {
            println!("== {stem} ==\n   {}", hits.join("\n   "));
        }
    }
}
