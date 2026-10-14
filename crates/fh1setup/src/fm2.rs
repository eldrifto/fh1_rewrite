//! `fh1setup import-fm2 <Forza Motorsport 2 ISO|zip|folder> [--data <dir>] [--only maps,cars]`: Forza Motorsport 2 (Xbox 360, 2007) circuits
//! into the ACTIVE install (`imported/fm2`), through the same builder as FM3's and FM4's ([`crate::fm4::build_maps`]).
//!
//! What FM2 shares with FM3 (checked on the disc): the folder layout (`Media/DB/gamedb.slt`, `Media/tracks/<track>/Ribbon_NN/*.col` collision,
//! `Media/AI/Tracks/<track>_NN.geo|.pit` data, `Media/StringTables/EN.zip`). What differs:
//! - `Tracks` has no `Environments` table / `EnvironmentId` / `StartLine` / `FinishLine` (the venue is the track folder name, every layout is
//!   a full lap) and `TrackType` 2 marks the reverse layouts (skipped like FM3's). `DebugTrack` / `TuningTrack` are inactive and skipped.
//! - `StartGridPositions` has one row per grid with a position and a heading (x, y, z) per slot: `PosX0, PosY0, PosZ0, HeadingX0, HeadingY0,
//!   HeadingZ0, ...` (46 rows, as many slots as the row fills).
//! - The scenery is converted by [`crate::fm2_scenery`] (the `.rmb.bin` models are CAFF containers with `.gpu` vertex / index
//!   buffers, a `.data` "rendergraph" and a recorded `.stream` of draw commands; `fh1_formats::fm2`): geometry and one diffuse
//!   texture per draw on a plain diffuse shader. Not decoded yet: lightmaps, blend masks, normal maps, the vertex format's
//!   normals, the winding state, props / LODs / zones, the UV scale (`FM2_UV_SCALE`, default 32). A scenery failure is reported
//!   and the circuit is drawn as its collision mesh.
//!
//! Paths are matched as the disc spells them, which only works on a case-insensitive filesystem (Windows) for now.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::fm4::{build_maps, Layout};

/// Grid slots per `StartGridPositions` row.
const SLOTS: usize = 16;

pub fn run() -> Result<()> {
    let mut it = std::env::args().skip(2);
    let (mut source, mut data, mut only) = (None, PathBuf::from("data"), None::<Vec<String>>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data needs a path")?.into(),
            "--only" => only = Some(it.next().context("--only needs maps,cars")?.split(',').map(str::to_owned).collect()),
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let source = source.context("usage: fh1setup import-fm2 <Forza Motorsport 2 ISO|zip|folder> [--data <dir>] [--only maps,cars]")?;
    let inst: Value = serde_json::from_slice(&std::fs::read(data.join("installation.json")).context("installation.json: run fh1setup on the FH1 disc first")?)?;
    println!("[fm2] disc: {}", source.display());
    let disc = crate::extract::resolve_any(&source, &data.join("work_fm2"))?;
    anyhow::ensure!(disc.join("media/db/gamedb.slt").exists(), "{}: no Media/DB/gamedb.slt (not an FM2 disc)", disc.display());
    let private = data.join("installations").join(inst["id"].as_str().context("installation.json id")?).join("assets/private");
    let root = private.join("imported/fm2");
    std::fs::create_dir_all(&root)?;
    let want = |p: &str| only.as_ref().is_none_or(|o| o.iter().any(|x| x == p));
    if want("maps") {
        let layouts = layouts(&disc)?;
        build_maps(&disc, &root, "fm2", "FM2", layouts)?;
        println!("[fm2] run `fh1setup <FH1 disc> --only remaster --force` to (re)build the FM2 remaster material tables");
    }
    if want("cars") {
        crate::fh2::swap(&root.join("cars"), |stage| crate::fm2_cars::cars(&disc, &private, stage))?;
    }
    println!("[fm2] done: {}", root.display());
    Ok(())
}

/// Every active forward layout of the gamedb whose `Ribbon_NN` folder exists (`FM2_TRACKS=sebring,...` limits the tracks).
fn layouts(disc: &std::path::Path) -> Result<Vec<Layout>> {
    let db = Connection::open_with_flags(disc.join("media/db/gamedb.slt"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let only: Option<Vec<String>> = std::env::var("FM2_TRACKS").ok().map(|v| v.split(',').map(|t| t.trim().to_ascii_lowercase()).collect());
    let mut stmt = db.prepare(
        "SELECT MediaName, RibbonIndex, DisplayName, Length, DefaultStartGridPositionsId, FlyingStartPosId, MediaName FROM Tracks \
         WHERE TrackType <> 2 AND Active = 1 ORDER BY MediaName, RibbonIndex",
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
                venue: venue_name(&r.get::<_, String>(6)?),
                start_line: 0.0,
                finish_line: 1.0,
                grid_slots: None,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<Layout> = Vec::new();
    for mut l in rows {
        let dup = out.iter().any(|o| o.track == l.track && o.ribbon == l.ribbon);
        if dup || only.as_ref().is_some_and(|o| !o.contains(&l.track.to_ascii_lowercase())) {
            continue;
        }
        if !disc.join("media/tracks").join(&l.track).join(format!("Ribbon_{:02}", l.ribbon)).is_dir() {
            println!("[fm2] {} ribbon {}: no Ribbon_{:02} folder", l.track, l.ribbon, l.ribbon);
            continue;
        }
        l.grid_slots = Some(grid_slots(&db, l.grid)?);
        out.push(l);
    }
    Ok(out)
}

/// The slots of `StartGridPositions` row `id`: ([x, height, z], [heading x, 0, heading z]), pole first; empty when the row is missing.
fn grid_slots(db: &Connection, id: i64) -> Result<Vec<([f32; 3], [f32; 3])>> {
    let cols: Vec<String> = (0..SLOTS)
        .flat_map(|i| [format!("PosX{i}"), format!("PosY{i}"), format!("PosZ{i}"), format!("HeadingX{i}"), format!("HeadingZ{i}")])
        .collect();
    let have: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('StartGridPositions')")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let cols: Vec<String> = cols.into_iter().filter(|c| have.contains(c)).collect();
    let sql = format!(
        "SELECT {} FROM StartGridPositions WHERE id = ?1",
        cols.iter().map(|c| format!("COALESCE({c}, 0.0)")).collect::<Vec<_>>().join(", ")
    );
    let vals: Option<Vec<f64>> = db.query_row(&sql, [id], |r| (0..cols.len()).map(|k| r.get::<_, f64>(k)).collect::<Result<Vec<f64>, _>>()).ok();
    let Some(v) = vals else { return Ok(Vec::new()) };
    Ok(v.chunks_exact(5)
        .filter(|s| s[0] != 0.0 || s[1] != 0.0 || s[2] != 0.0)
        .map(|s| ([s[0] as f32, s[1] as f32, s[2] as f32], [s[3] as f32, 0.0, s[4] as f32]))
        .collect())
}

/// The venue shown for a track folder name (FM2's gamedb has no venue table): `LagunaSeca` -> `Laguna Seca`, with the venues whose
/// folder name is not the in-game one spelled out.
fn venue_name(media: &str) -> String {
    match media.to_ascii_lowercase().as_str() {
        "sunsetinfield" => return "Sunset Peninsula".into(),
        "testtrackinfield" => return "Test Track".into(),
        _ => {}
    }
    let mut out = String::new();
    for (i, c) in media.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}
