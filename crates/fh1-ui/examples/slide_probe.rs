//! slide_probe <scene_dir> <SCENE> [slide] [event...] — go to a scene-level slide (and fire events), then print the
//! visible draws (name, opacity, position, text). Used to see which slide/event reveals a menu's objects.

use fh1_ui::anark::Scene;
use fh1_ui::player::{DrawKind, Player};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let base = std::path::Path::new(&a[1]).join(a[2].to_lowercase());
    let read = |e: &str| std::fs::read(base.with_extension(e)).ok();
    let mut p = Player::new(Scene::load(&read("bgf").unwrap(), read("fbf").as_deref(), read("bsg").as_deref()).unwrap());
    if let Some(slide) = a.get(3) {
        println!("goto_slide(Scene, {slide}) = {}", p.goto_slide(0, slide));
    }
    for spec in a.iter().skip(4) {
        if let Some((obj, slide)) = spec.split_once('=') {
            if let Ok(i) = obj.parse::<usize>() {
                println!("goto_slide({i}, {slide}) = {}", p.goto_slide(i, slide));
                continue;
            }
            if let Some(i) = p.find(obj) {
                println!("goto_slide({obj}#{i}, {slide}) = {}", p.goto_slide(i, slide));
                continue;
            }
            println!("fire({spec}) = {} actions", p.fire(spec));
        }
        // text:<object>:<string>
        if let Some(rest) = spec.strip_prefix("text:") {
            if let Some((obj, s)) = rest.split_once(':') {
                if let Ok(i) = obj.parse::<usize>() {
                    p.set_text(i, s.to_string());
                    println!("set_text({i}, {s:?})");
                }
            }
        }
        if let Some(rest) = spec.strip_prefix("set:") {
            let (obj, kv) = rest.split_once(':').unwrap_or((rest, ""));
            let (prop, val) = kv.split_once('=').unwrap_or((kv, "100"));
            if let (Ok(i), Ok(v)) = (obj.parse::<usize>(), val.parse::<f32>()) {
                if prop == "opacity" {
                    p.set(i, fh1_ui::names::OPACITY, fh1_ui::player::Value::Float(v));
                    println!("set({i}, opacity={v})");
                }
            }
        }
    }
    p.update(500.0);
    let f = p.evaluate();
    let visible: Vec<_> = f.draws.iter().filter(|d| d.opacity > 0.02).collect();
    println!("draws: {} ({} visible)", f.draws.len(), visible.len());
    for d in visible.iter().take(40) {
        let cam = f.camera(d.layer);
        let (x, y) = (d.world[0][3] - cam.position[0], d.world[1][3] - cam.position[1]);
        let ortho = cam.orthographic;
        match &d.kind {
            DrawKind::Text(t) => println!("  {:<26} op={:.2} ({x:.0},{y:.0}) layer={} ortho={ortho} {:?}", d.name, d.opacity, d.layer, t.string),
            DrawKind::Model { materials, .. } => {
                let op = materials.iter().map(|m| m.opacity).fold(0.0f32, f32::max);
                let tex: usize = materials.iter().map(|m| m.textures.len()).sum();
                let sx = (d.world[0][0] * d.world[0][0] + d.world[1][0] * d.world[1][0]).sqrt();
                let sy = (d.world[0][1] * d.world[0][1] + d.world[1][1] * d.world[1][1]).sqrt();
                let add = materials.iter().any(|m| m.additive);
                println!("  {:<26} op={:.2} mat={op:.2} tex={tex} add={add} scale=({sx:.2},{sy:.2}) ({x:.0},{y:.0}) layer={} ortho={ortho}", d.name, d.opacity, d.layer);
            }
        }
    }
}
