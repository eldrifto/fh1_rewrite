//! `fh1setup import-fm3 <Play Disc ISO|zip|folder> [--disc2 <Content Install Disc>] [--data <dir>] [--only maps]`: Forza Motorsport 3 (Xbox 360) circuits into the
//! ACTIVE install (`imported/fm3`), through the same builder as FM4's ([`crate::fm4::build_maps`]).
//!
//! What FM3 shares with FM4 (checked on the Play Disc): the gamedb schema for the tables the importers read, the
//! `Media/Tracks/<track>/Ribbon_NN/*.col` collision (embedded squares, `fh1_world::World::from_col` builds all 20 tracks), the
//! `Media/AI/Tracks/<track>_NN.geo|.pit` "MLP" data (same fields: centreline every 2 m, normals, walls, optimal line) and the
//! English strings (`Media/StringTables/EN.zip`). What differs:
//! - `Tracks` has `TrackType` (1 = forward, 2 = the reverse layout, 0 = a layout without a reverse), no `IsReverse` /
//!   `IsHomespace`; reverse layouts are skipped here like FM4's.
//! - `StartGridPositions` holds one row per grid with up to 12 absolute slots (`PosX0..`, `PosY0..`, `PosZ0..`,
//!   `HeadingX0..`): position = (x, height, z) of the collision space, heading = (x, z) of the direction of travel (VERIFIED
//!   against the `.geo` centreline on four tracks: slot 0 is 2-3 m from a waypoint, the heading matches its tangent).
//! - The scenery converts with the FM4 code after three small format changes, all in the headers: the track root `.pvs` is
//!   version 0x18 (FM4's 0x1B; the same binding tables, found by the same validation), the `.rmb.bin` models are
//!   CTrackModel version 4 (6 in FH1 / FM4; the submodel layout is the same) and the `.bix` textures start `BIX0` (FM4:
//!   `BIX1`, the same header fields). A track whose scenery fails to convert is reported and drawn as its collision mesh.
//! - Two more things the FM4 code did not need, both in the track's `bin.zip` / `.pvs`:
//!   - **`<track>.stx.bin`**: a CAFF with one 128-byte `"texture"` descriptor per PVS texture index and every texture's Xenos
//!     chain back to back in `.gpu` ([`fh1_formats::caff::parse_stx`]; the entry sizes sum to the `.gpu` length on all 20
//!     tracks). Entry `i` is the data of PVS texture `i` when the PVS flags it as a runtime texture (flags exactly 1): the
//!     grass and chain-link-fence diffuse / opacity, reflection cubes. Without it those drew with the default white texture:
//!     white patches over the terrain (the `grass_*` overlays) and solid white walls where fences stand. Tracks whose stx does
//!     not parse fall back to skipping the textureless grass overlays ("grass-overlay submodels skipped" line). The
//!     lightmap atlas pages (PVS flags 9 tiles: `Texture::object` = the page's PVS index, `Texture::uv` = scale + offset in
//!     the page) are in it too: each tile is cropped out as a texture of its own.
//!   - **The skid-mark mask**: the road shaders lerp `RoadSkid` -> `RoadSkid2` by tf11, a runtime render target. FM3's
//!     `RoadSkid2` is ~1.5 (FM4: ~0.2), so the unbound (white) default made every road glare like freshly rubbered-in
//!     tarmac; `index.json` `"skid_mask": "black"` makes fh1-render bind black (no marks) there.
//!   - **The instance table** ([`fh1_formats::pvs::fm3_instances`], `scenery::fm3_placements`): 56-byte records after the
//!     shader list place every model in the world. The origin-centred models (houses, telegraph poles and pylons, trees, signs,
//!     parked cars, cones: 45-91 templates per track) only exist as templates, so without their instances the tracks had
//!     empty villages and bare roadsides (Fujimi Kaido's right-hand side was a hole in the map). They go through the props
//!     path; the LOD1 / LOD2 variants of a tree sit at the same spot as its LOD0 and are left out (no LOD chain yet).
//! - Amalfi (Rally di Positano), Fujimi Kaido and the Nurburgring (and their stages) aren't on the Play Disc: without
//!   `--disc2` their `Ribbon_NN` folders are missing and they are skipped ("no Ribbon" lines). The Content Install Disc holds them
//!   in its "Road Racing Track Pack" STFS package (`Content/0000000000000000/4D53084D/00000002/4D53084D0CCF0060`, unpacked with
//!   the FM4 reader to `work_fm3_disc2/content`): loose `.col` collision and `.geo`/`.irt`/`.seg` AI data, a pack zip with the
//!   `.pvs` / `.crowd` / camera files, and one `<Track>_pri_65.zip` per track (the Play Disc's `bin.zip`: models, textures,
//!   shaders; `zipmount.xml` says where each mounts). [`merge_content`] lays all of that over the Play Disc's `media` as
//!   `work_fm3_disc2/merged` and the importer runs on it unchanged (69 layouts instead of 52; the reverse layouts are skipped
//!   as before). The three car packs on the disc (Asian / European / American) hold `.carbin` cars: not imported yet.
//!   Cars aren't imported yet (the `.carbin` models are another format).
//!
//! Paths are matched as the disc spells them (`Media/DB/gamedb.slt` vs FM4's `media/db`), which only works on a
//! case-insensitive filesystem (Windows) for now.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::fm4::{build_maps, Layout};

/// Grid slots per `StartGridPositions` row.
const SLOTS: usize = 12;

pub fn run() -> Result<()> {
    let mut it = std::env::args().skip(2);
    let (mut source, mut data, mut only, mut disc2) = (None, PathBuf::from("data"), None::<Vec<String>>, None::<PathBuf>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data needs a path")?.into(),
            "--disc2" => disc2 = Some(it.next().context("--disc2 needs the Content Install Disc")?.into()),
            "--only" => only = Some(it.next().context("--only needs maps")?.split(',').map(str::to_owned).collect()),
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let source = source.context("usage: fh1setup import-fm3 <Play Disc ISO|zip|folder> [--disc2 <Content Install Disc>] [--data <dir>] [--only maps]")?;
    let inst: Value = serde_json::from_slice(&std::fs::read(data.join("installation.json")).context("installation.json: run fh1setup on the FH1 disc first")?)?;
    println!("[fm3] Play Disc: {}", source.display());
    let disc = crate::extract::resolve_any(&source, &data.join("work_fm3_disc1"))?;
    anyhow::ensure!(disc.join("media/db/gamedb.slt").exists(), "{}: no Media/DB/gamedb.slt (not an FM3 Play Disc)", disc.display());
    let private = data.join("installations").join(inst["id"].as_str().context("installation.json id")?).join("assets/private");
    let root = private.join("imported/fm3");
    std::fs::create_dir_all(&root)?;
    let disc = match &disc2 {
        Some(d2) => {
            let work = data.join("work_fm3_disc2");
            let content = unpack_content(&crate::extract::resolve_any(d2, &work)?, &work)?;
            merge_content(&disc, &content, &work)?
        }
        None => disc,
    };
    let want = |p: &str| only.as_ref().is_none_or(|o| o.iter().any(|x| x == p));
    if want("maps") {
        let layouts = layouts(&disc)?;
        build_maps(&disc, &root, "fm3", "FM3", layouts)?;
    }
    println!("[fm3] done: {}", root.display());
    Ok(())
}

/// Every forward layout of the gamedb whose `Ribbon_NN` folder exists (`FM3_TRACKS=sebring,...` limits the tracks).
fn layouts(disc: &std::path::Path) -> Result<Vec<Layout>> {
    let db = Connection::open_with_flags(disc.join("media/db/gamedb.slt"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let only: Option<Vec<String>> = std::env::var("FM3_TRACKS").ok().map(|v| v.split(',').map(|t| t.trim().to_ascii_lowercase()).collect());
    let mut stmt = db.prepare(
        "SELECT t.MediaName, t.RibbonIndex, t.DisplayName, t.Length, t.DefaultStartGridPositionsId, t.FlyingStartPosId, \
         COALESCE(e.DisplayName, t.MediaName), t.StartLine, t.FinishLine FROM Tracks t LEFT JOIN Environments e ON e.Id = t.EnvironmentId \
         WHERE t.TrackType <> 2 ORDER BY t.MediaName, t.RibbonIndex",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Layout {
                track: r.get(0)?,
                ribbon: r.get::<_, i64>(1)? as u32,
                name: r.get(2)?,
                length: r.get(3)?,
                grid: r.get(4)?,
                flying: r.get(5)?,
                venue: r.get(6)?,
                start_line: r.get(7)?,
                finish_line: r.get(8)?,
                grid_slots: None,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<Layout> = Vec::new();
    for mut l in rows {
        let dup = out.iter().any(|o| o.track == l.track && o.ribbon == l.ribbon);
        let ui = l.track.to_ascii_lowercase().starts_with("ui");
        if dup || ui || only.as_ref().is_some_and(|o| !o.contains(&l.track.to_ascii_lowercase())) {
            continue;
        }
        if !disc.join("media/tracks").join(&l.track).join(format!("Ribbon_{:02}", l.ribbon)).is_dir() {
            println!("[fm3] {} ribbon {}: no Ribbon_{:02} folder (not on the Play Disc)", l.track, l.ribbon, l.ribbon);
            continue;
        }
        l.grid_slots = Some(grid_slots(&db, l.grid)?);
        out.push(l);
    }
    Ok(out)
}

/// The slots of `StartGridPositions` row `id`: ([x, height, z], [heading x, 0, heading z]), pole first; empty when the row
/// is missing (the layout is then reported as having no grid slot).
fn grid_slots(db: &Connection, id: i64) -> Result<Vec<([f32; 3], [f32; 3])>> {
    let cols: Vec<String> = (0..SLOTS).flat_map(|i| [format!("PosX{i}"), format!("PosY{i}"), format!("PosZ{i}"), format!("HeadingX{i}"), format!("HeadingZ{i}")]).collect();
    let sql = format!("SELECT {} FROM StartGridPositions WHERE id = ?1", cols.iter().map(|c| format!("COALESCE({c}, 0.0)")).collect::<Vec<_>>().join(", "));
    let vals: Option<Vec<f64>> = db
        .query_row(&sql, [id], |r| (0..cols.len()).map(|k| r.get::<_, f64>(k)).collect::<Result<Vec<f64>, _>>())
        .ok();
    let Some(v) = vals else { return Ok(Vec::new()) };
    Ok(v.chunks_exact(5)
        .filter(|s| s[0] != 0.0 || s[1] != 0.0 || s[2] != 0.0)
        .map(|s| ([s[0] as f32, s[1] as f32, s[2] as f32], [s[3] as f32, 0.0, s[4] as f32]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_with_grid() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        let cols: Vec<String> = (0..SLOTS).flat_map(|i| [format!("PosX{i}"), format!("PosY{i}"), format!("PosZ{i}"), format!("HeadingX{i}"), format!("HeadingY{i}"), format!("HeadingZ{i}")]).collect();
        db.execute(&format!("CREATE TABLE StartGridPositions (id INTEGER, {})", cols.iter().map(|c| format!("{c} REAL")).collect::<Vec<_>>().join(", ")), []).unwrap();
        // Slot 0 and 1 filled, slot 2 all zero, the rest NULL (a grid of two).
        db.execute("INSERT INTO StartGridPositions (id, PosX0, PosY0, PosZ0, HeadingX0, HeadingZ0, PosX1, PosY1, PosZ1, HeadingX1, HeadingZ1, PosX2, PosY2, PosZ2) VALUES (7, -345.6, 0.9, 281.0, -0.6, -0.8, 4.0, 5.0, 6.0, 1.0, 0.0, 0.0, 0.0, 0.0)", []).unwrap();
        db
    }

    #[test]
    fn grid_slots_are_absolute_pole_first_and_skip_empty_ones() {
        let slots = grid_slots(&db_with_grid(), 7).unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0], ([-345.6, 0.9, 281.0], [-0.6, 0.0, -0.8]));
        assert_eq!(slots[1], ([4.0, 5.0, 6.0], [1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_missing_grid_row_gives_no_slots() {
        assert!(grid_slots(&db_with_grid(), -1).unwrap().is_empty());
    }
}

/// Unpack the Content Install Disc's STFS packages (`Content/0000000000000000/4D53084D/00000002/*`) to `work/content/<package>/`.
fn unpack_content(disc2: &std::path::Path, work: &std::path::Path) -> Result<PathBuf> {
    use crate::fm4_merge::Stfs;
    use std::io::Write;
    let pkgdir = ["content", "0000000000000000", "4d53084d", "00000002"].iter().fold(disc2.to_path_buf(), |p, n| {
        std::fs::read_dir(&p).ok().and_then(|d| d.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(n))).map_or(p.join(n), |e| e.path())
    });
    anyhow::ensure!(pkgdir.is_dir(), "{}: no Content/0000000000000000/4D53084D/00000002 (not the Forza Motorsport 3 Content Install Disc)", disc2.display());
    let mut names: Vec<String> = std::fs::read_dir(&pkgdir)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for pkg in names {
        let out = work.join("content").join(&pkg);
        if out.join(".extracted").exists() {
            continue;
        }
        if out.exists() {
            std::fs::remove_dir_all(&out)?;
        }
        let st = Stfs::open(&pkgdir.join(&pkg))?;
        let entries = st.entries()?;
        println!("[fm3] unpacking {pkg} ({}, {} files)", st.name, entries.iter().filter(|e| !e.dir).count());
        for e in entries.iter().filter(|e| !e.dir) {
            let p = out.join(&e.path);
            std::fs::create_dir_all(p.parent().unwrap())?;
            let mut f = std::io::BufWriter::new(std::fs::File::create(&p)?);
            st.read(e, &mut f).with_context(|| format!("{pkg}: {}", e.path))?;
            f.flush()?;
        }
        std::fs::write(out.join(".extracted"), "")?;
    }
    Ok(work.join("content"))
}

/// `work/merged`: the Play Disc's `media` with the Content Install Disc's Road Racing Track Pack laid over `tracks` and `AI/Tracks`
/// (everything else is a junction to the Play Disc, so nothing is copied but the pack's own files):
/// - `<pkg>/Media/DLCZips/<name>_pri_65/Media/{Tracks,AI/Tracks}/...` (loose: the `.col` collision and the `.geo`/`.irt`/`.seg` AI data),
/// - `<pkg>/Media/DLCZips/<name>_pri_65.zip`'s `media/Tracks/...` (the `.pvs` bindings, `.crowd`, camera XML; the zip that has a
///   loose twin, `0060_pri_65.zip`) and
/// - `<Track>_pri_65.zip` mounted by `zipmount.xml` at `Media\Tracks\<Track>\bin\`: the track's models, textures and shaders,
///   which is the `bin.zip` the Play Disc keeps in each track folder.
fn merge_content(disc1: &std::path::Path, content: &std::path::Path, work: &std::path::Path) -> Result<PathBuf> {
    use crate::fm4_merge::{child_ci, clear, link_dir, link_or_copy, sorted_names};
    use std::path::Path;
    let src = child_ci(disc1, "media");
    let root = work.join("merged");
    let m = root.join("media");
    let source = format!("{}\n{}", std::path::absolute(disc1)?.display(), std::path::absolute(content)?.display());
    let marker = root.join(".source");
    if root.exists() && std::fs::read_to_string(&marker).ok().as_deref() != Some(source.as_str()) {
        println!("[fm3] rebuilding {} (different discs)", root.display());
        clear(&m)?;
    }
    std::fs::create_dir_all(&m)?;
    std::fs::write(&marker, &source)?;

    // Play Disc: junctions, except `Tracks` and `AI`, whose folders get the pack's files too.
    let link = |s: &Path, d: &Path| -> Result<()> {
        if d.symlink_metadata().is_err() {
            if s.is_dir() {
                link_dir(s, d)?;
            } else {
                link_or_copy(s, d)?;
            }
        }
        Ok(())
    };
    for n in sorted_names(&src)? {
        let lower = n.to_ascii_lowercase();
        if lower != "tracks" && lower != "ai" {
            link(&src.join(&n), &m.join(&n))?;
        }
    }
    let link_children = |from: &Path, to: &Path| -> Result<()> {
        std::fs::create_dir_all(to)?;
        for n in sorted_names(from)? {
            link(&from.join(&n), &to.join(&n))?;
        }
        Ok(())
    };
    link_children(&child_ci(&src, "tracks"), &m.join("tracks"))?;
    let (ai_src, ai_dst) = (child_ci(&src, "ai"), m.join("ai"));
    std::fs::create_dir_all(&ai_dst)?;
    for n in sorted_names(&ai_src)? {
        if !n.eq_ignore_ascii_case("tracks") {
            link(&ai_src.join(&n), &ai_dst.join(&n))?;
        }
    }
    // The AI track files are linked one by one (the pack adds to the same folder).
    let ai_tracks = ai_dst.join("tracks");
    std::fs::create_dir_all(&ai_tracks)?;
    for n in sorted_names(&child_ci(&ai_src, "tracks"))? {
        link(&child_ci(&ai_src, "tracks").join(&n), &ai_tracks.join(&n))?;
    }

    // Content Install Disc.
    fn overlay(from: &Path, to: &Path) -> Result<usize> {
        use crate::fm4_merge::{link_or_copy, sorted_names};
        std::fs::create_dir_all(to)?;
        let mut n = 0;
        for name in sorted_names(from)? {
            let (s, d) = (from.join(&name), to.join(&name));
            if s.is_dir() {
                n += overlay(&s, &d)?;
            } else if !d.exists() {
                link_or_copy(&s, &d)?;
                n += 1;
            }
        }
        Ok(n)
    }
    let (mut loose, mut zipped, mut bins) = (0, 0, 0);
    for pkg in sorted_names(content)? {
        let dlc = child_ci(&child_ci(&content.join(&pkg), "media"), "dlczips");
        if !dlc.is_dir() {
            continue;
        }
        let names = sorted_names(&dlc)?;
        for n in &names {
            let p = dlc.join(n);
            if p.is_dir() {
                let media = child_ci(&p, "media");
                for part in [Path::new("tracks"), Path::new("ai/tracks")] {
                    let from = part.iter().fold(media.clone(), |d, c| child_ci(&d, &c.to_string_lossy()));
                    if from.is_dir() {
                        loose += overlay(&from, &m.join(part))?;
                    }
                }
            }
        }
        // `zipmount.xml`: `<zip Name="X.zip" Mount="game:\Media\Tracks\Y\bin\" .../>`.
        let mount = std::fs::read_to_string(child_ci(&dlc, "zipmount.xml")).unwrap_or_default();
        for tag in mount.split("<zip ").skip(1) {
            let attr = |k: &str| tag.split(&format!("{k}=\"")).nth(1).and_then(|r| r.split('"').next()).map(str::to_owned);
            let (Some(name), Some(at)) = (attr("Name"), attr("Mount")) else { continue };
            let parts: Vec<&str> = at.trim_end_matches('\\').split('\\').collect();
            if let [_, media, tracks, track, bin] = parts[..] {
                if media.eq_ignore_ascii_case("media") && tracks.eq_ignore_ascii_case("tracks") && bin.eq_ignore_ascii_case("bin") {
                    let dst = m.join("tracks").join(track);
                    std::fs::create_dir_all(&dst)?;
                    if !dst.join("bin.zip").exists() {
                        link_or_copy(&child_ci(&dlc, &name), &dst.join("bin.zip"))?;
                        bins += 1;
                    }
                }
            }
        }
        // The pack's own zip: its `media/Tracks/...` files.
        for n in &names {
            if !n.to_ascii_lowercase().ends_with("_pri_65.zip") || !dlc.join(n.trim_end_matches(".zip")).is_dir() {
                continue;
            }
            let mut ar = fh1_formats::zip::Archive::open(dlc.join(n)).with_context(|| n.clone())?;
            for e in ar.entries.clone() {
                let name = e.name.replace('\\', "/");
                let Some(rest) = name.get(..13).filter(|h| h.eq_ignore_ascii_case("media/tracks/")).map(|_| &name[13..]) else { continue };
                let dst = m.join("tracks").join(rest);
                if rest.is_empty() || rest.ends_with('/') || dst.exists() {
                    continue;
                }
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::write(&dst, ar.read(&e).with_context(|| format!("{n}: {}", e.name))?)?;
                zipped += 1;
            }
        }
    }
    println!("[fm3] merged root {} (Content Install Disc: {loose} loose files, {zipped} from the pack zip, {bins} track bin.zip)", root.display());
    Ok(root)
}
