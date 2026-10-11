//! `fh1setup import-fm3 <Play Disc ISO|zip|folder> [--data <dir>] [--only maps]`: Forza Motorsport 3 (Xbox 360) circuits into the
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
//! - The scenery index (`.pvs`) is version 0x18 (FM4's is 0x1B): not converted yet, so a map without `scenery/` is drawn
//!   as its collision mesh (the engine's fallback).
//! - Amalfi, Fujimi Kaido and the Nurburgring (and their stages) aren't on the Play Disc: their `Ribbon_NN` folders are
//!   missing and they are skipped ("no Ribbon" lines). Cars aren't imported yet (the `.carbin` models are another format).
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
    let (mut source, mut data, mut only) = (None, PathBuf::from("data"), None::<Vec<String>>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data needs a path")?.into(),
            "--only" => only = Some(it.next().context("--only needs maps")?.split(',').map(str::to_owned).collect()),
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let source = source.context("usage: fh1setup import-fm3 <Play Disc ISO|zip|folder> [--data <dir>] [--only maps]")?;
    let inst: Value = serde_json::from_slice(&std::fs::read(data.join("installation.json")).context("installation.json: run fh1setup on the FH1 disc first")?)?;
    println!("[fm3] Play Disc: {}", source.display());
    let disc = crate::extract::resolve_any(&source, &data.join("work_fm3_disc1"))?;
    anyhow::ensure!(disc.join("media/db/gamedb.slt").exists(), "{}: no Media/DB/gamedb.slt (not an FM3 Play Disc)", disc.display());
    let private = data.join("installations").join(inst["id"].as_str().context("installation.json id")?).join("assets/private");
    let root = private.join("imported/fm3");
    std::fs::create_dir_all(&root)?;
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
