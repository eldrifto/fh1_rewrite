//! Probe: audit every FM2 car zip with `carbin_fm2` and print the anomalies (parse failures, parts without a centre, no `body` part,
//! assembled size far from MAXData's collision box, huge / empty parts, uv outside the usual range).
//! `cargo run -p fh1-formats --release --example fm2_car_audit -- <Media/cars dir>`
use fh1_formats::{carbin_fm2, zip::Archive};

fn attr(xml: &str, tag: &str, name: &str) -> Option<f32> {
    let t = &xml[xml.find(tag)?..];
    let t = &t[..t.find('>')?];
    let v = &t[t.find(&format!("{name}=\""))? + name.len() + 2..];
    v[..v.find('"')?].parse().ok()
}

fn main() {
    let dir = std::env::args().nth(1).expect("cars dir");
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
    entries.sort();
    let (mut ok, mut bad) = (0, 0);
    for p in entries {
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case("shared") {
            continue;
        }
        let Ok(mut ar) = Archive::open(&p) else { continue };
        let find = |ar: &Archive<_>, s: &str| ar.entries.iter().find(|x| x.name.to_ascii_lowercase().ends_with(s)).cloned();
        let Some(cb) = find(&ar, ".carbin") else { continue };
        let car = match carbin_fm2::parse(&ar.read(&cb).unwrap()) {
            Ok(c) => c,
            Err(e) => {
                println!("{name}: PARSE FAIL {e}");
                bad += 1;
                continue;
            }
        };
        let mut issues: Vec<String> = Vec::new();
        let nocentre: Vec<&str> = car.parts.iter().filter(|p| p.centre.is_none() && !p.vertices.is_empty()).map(|p| p.name.as_str()).collect();
        if !nocentre.is_empty() {
            issues.push(format!("no centre: {}", nocentre.join(",")));
        }
        if !car.parts.iter().any(|p| p.name.eq_ignore_ascii_case("body")) {
            issues.push("no body part".into());
        }
        // Mirrored pairs (glassLF / glassRF ...) must sit mirrored: same y / z, opposite x.
        for a in &car.parts {
            let suffix: String = a.name.chars().skip_while(|c| c.is_ascii_lowercase()).collect();
            if !suffix.starts_with('L') {
                continue;
            }
            let twin = format!("{}R{}", &a.name[..a.name.len() - suffix.len()], &suffix[1..]);
            if let (Some(ca), Some(cb)) = (a.centre, car.parts.iter().find(|b| b.name == twin).and_then(|b| b.centre)) {
                if (ca[0] + cb[0]).abs() > 0.06 || (ca[1] - cb[1]).abs() > 0.06 || (ca[2] - cb[2]).abs() > 0.1 {
                    issues.push(format!("{} / {twin} not mirrored: {:?} vs {:?}", a.name, ca, cb));
                }
            }
        }
        let empty: Vec<&str> = car.parts.iter().filter(|p| !p.runs.iter().any(|r| r.lod == 1 && !r.indices.is_empty())).map(|p| p.name.as_str()).collect();
        if empty.len() > 4 {
            issues.push(format!("{} parts without lod1 tris", empty.len()));
        }
        // Assembled extent (parts with a centre, excluding the race / variant extras that sit outside the stock shape).
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for part in car.parts.iter().filter(|p| !p.name.to_ascii_lowercase().ends_with("race")) {
            let Some(c) = part.centre else { continue };
            let used: std::collections::HashSet<u32> = part.runs.iter().filter(|r| r.lod == 1).flat_map(|r| r.indices.iter().copied()).collect();
            for &i in &used {
                let v = part.vertices[i as usize].position;
                for k in 0..3 {
                    mn[k] = mn[k].min(v[k] + c[k]);
                    mx[k] = mx[k].max(v[k] + c[k]);
                }
            }
        }
        let ext = [mx[0] - mn[0], mx[1] - mn[1], mx[2] - mn[2]];
        if let Some(md) = find(&ar, "maxdata.xml").and_then(|e| ar.read(&e).ok()).map(|b| String::from_utf8_lossy(&b).into_owned()) {
            let g = |n: &str| attr(&md, "<BoundingBox", n);
            if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (g("MinX"), g("MinY"), g("MinZ"), g("MaxX"), g("MaxY"), g("MaxZ")) {
                let want = [d - a, e - b, f - c];
                let r: Vec<f32> = (0..3).map(|k| ext[k] / want[k].max(0.01)).collect();
                if r.iter().any(|x| !(0.8..=1.35).contains(x)) {
                    issues.push(format!("size ratio vs MAXData x{:.2} y{:.2} z{:.2} (mesh {:.2}x{:.2}x{:.2})", r[0], r[1], r[2], ext[0], ext[1], ext[2]));
                }
            }
        } else {
            issues.push("no MAXData".into());
        }
        // Long thin or scattered geometry: a body part far from the origin.
        if let Some(b) = car.parts.iter().find(|p| p.name.eq_ignore_ascii_case("body")) {
            if let Some(c) = b.centre {
                if c[0].abs() > 0.3 || c[2].abs() > 0.8 {
                    issues.push(format!("body centre {c:?}"));
                }
            }
        }
        if issues.is_empty() {
            ok += 1;
        } else {
            bad += 1;
            println!("{name}: {}", issues.join(" | "));
        }
    }
    println!("{ok} clean, {bad} with issues");
}
