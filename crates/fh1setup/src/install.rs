//! installation.json bookkeeping and per-group staging.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{cars, extract::hex, GROUPS};

#[derive(Serialize, Deserialize, Default)]
pub struct Installation {
    /// Directory name under `installations/`.
    pub id: String,
    pub source: String,
    /// SHA-256 of default.xex.
    pub source_hash: String,
    /// group -> pipeline hash of the output currently on disk.
    pub pipelines: BTreeMap<String, String>,
}

/// Fingerprint of the environment knobs that change the importer's **output**.
///
/// The pipeline hash used to cover the group name and its version string only, so an install built with one set
/// of `FM2_*` knobs and re-imported with another produced **different content under the same hash**. Measured
/// (MODLOG 303): re-importing Sebring with default settings, against an install built with an unknown earlier
/// configuration, changed the rendered frame by **mean 2.52 / 29 % of pixels** -- 100x the capture noise -- while
/// two imports with the *same* environment agree to 0.04. So the content is deterministic given the environment and
/// not otherwise.
///
/// Every set `FM2_*` / `FH1_*` variable is included except a small deny-list of read-only diagnostics, so a new
/// knob cannot be forgotten. Any such knob that is unset everywhere keeps the old hash (empty fingerprint), as
/// does anything on the deny-list, so ordinary imports are unaffected.
pub fn env_fingerprint() -> String {
    const DIAGNOSTIC: &[&str] = &[
        "FM2_TRACKS", "FM2_UNDER", "FM2_UNDER_TOL", "FM2_DEBUG", "FM2_AT", "FM2_NEAR", "FM2_SHOW_MODEL",
        "FM2_HIDE_MODELS", "FM2_DUMP_MODEL", "FM2_DUMP_PS", "FM2_PROBE_AT", "FM2_PROBE_TOL", "FM2_PROBE_LERP",
        "FM2_PROBE_GAMMA", "FM2_PROBE_WHITE_ID", "FM2_PROBE_WHITE_LM", "FM2_PROBE_BIND", "FM2_PROBE_TEXT",
        "FM2_PROBE_STATE", "FM2_PROBE_TEX", "FM2_ATLAS_MODEL", "FM2_UV_MODEL", "FM2_PS_LIST", "FM2_STREAM_TYPES",
        "FM2_NAMES", "FM2_FIND_SIG", "FM2_FIND_TEX", "FM2_PAIRS_MODEL", "FM2_SCAN_WORD", "FM2_PATTERN_AT",
        "FM2_MARK_DIM", "FM2_SHADE", "FM2_SKY_GAIN",
        // Read-only diagnostics: they print, they do not change what is written.
        "FM2_LM_STATS", "FM2_LM_SEAMS", "FM2_LM_RINGS", "FM2_DEBUG_DRAW", "FM2_SKY_MAT",
    ];
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| (k.starts_with("FM2_") || k.starts_with("FH1_")) && !DIAGNOSTIC.contains(&k.as_str()))
        .collect();
    vars.sort();
    if vars.is_empty() {
        return String::new();
    }
    let mut h = Sha256::new();
    for (k, v) in &vars {
        h.update(k.as_bytes());
        h.update([b'=']);
        h.update(v.as_bytes());
        h.update([0]);
    }
    format!(" env{}", hex(&h.finalize())[..8].to_owned())
}

fn pipeline_hash(group: &str, version: &str) -> String {
    let mut h = Sha256::new();
    h.update(group.as_bytes());
    h.update([0]);
    h.update(version.as_bytes());
    // MODLOG 303: the environment that produced the output is part of the pipeline's identity.
    h.update([0]);
    h.update(env_fingerprint().as_bytes());
    hex(&h.finalize())[..16].to_owned()
}

pub fn run(disc: &Path, data: &Path, force: bool, only: Option<&[String]>) -> Result<()> {
    let xex = std::fs::read(disc.join("default.xex")).context("reading default.xex")?;
    let source_hash = hex(&Sha256::digest(&xex));
    let id = source_hash[..16].to_owned();

    let manifest_path = data.join("installation.json");
    let mut inst: Installation = std::fs::read(&manifest_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|i: &Installation| i.source_hash == source_hash)
        .unwrap_or_default();
    inst.id = id.clone();
    inst.source = disc.display().to_string();
    inst.source_hash = source_hash;

    let private = data.join("installations").join(&id).join("assets").join("private");
    std::fs::create_dir_all(&private)?;

    let selected: Vec<_> = GROUPS.iter().filter(|(g, _)| only.is_none_or(|o| o.iter().any(|x| x == g))).collect();
    for (i, &&(group, version)) in selected.iter().enumerate() {
        // Machine-readable progress for the launcher (fh1-launcher): `[progress] <done>/<total> <group>`.
        println!("[progress] {i}/{} {group}", selected.len());
        let hash = pipeline_hash(group, version);
        let out = private.join(group);
        if !force && out.is_dir() && inst.pipelines.get(group) == Some(&hash) {
            println!("[{group}] up to date");
            continue;
        }
        // Build into a staging folder, then swap, so a failed run never leaves half a group.
        let stage = private.join(format!("{group}.staging"));
        if stage.exists() {
            std::fs::remove_dir_all(&stage)?;
        }
        std::fs::create_dir_all(&stage)?;
        // The audio group skips WAVs that already exist: seed the stage with the installed WAVs (hard links, copy
        // fallback) so a version bump decodes only the new banks. Only *.wav: the JSON is rewritten through fs::write,
        // which would write through a hard link into the installed group.
        if group == "audio" && out.join("banks").is_dir() {
            if let Err(e) = seed_wavs(&out.join("banks"), &stage.join("banks")) {
                println!("[audio] could not reuse decoded banks ({e:#}); decoding everything");
            }
        }
        println!("[{group}] building ({version})");
        match group {
            "cars" => cars::build(disc, &stage)?,
            "world" => crate::world::build(disc, &stage)?,
            "scenery" => crate::scenery::build(disc, &stage)?,
            "shaders" => crate::shaders::build(disc, &stage)?,
            "tracks" => crate::tracks::build(disc, &stage)?,
            "dynamicpost" => crate::dynamicpost::build(disc, &stage)?,
            "grass" => crate::grass::build(disc, &stage)?,
            "crowd" => crate::crowd::build(disc, &stage)?,
            "anim" => crate::anim::build(disc, &stage)?,
            "sky" => crate::sky::build(disc, &stage)?,
            "camera" => crate::camera::build(disc, &stage)?,
            "effects" => crate::effects::build(disc, &stage)?,
            "events" => crate::events::build(disc, &stage)?,
            "ailines" => crate::ailines::build(disc, &stage)?,
            "traffic" => crate::traffic::build(disc, &stage)?,
            "upgrades" => crate::upgrades::build(disc, &stage)?,
            "variants" => crate::variants::build(disc, &stage)?,
            "remaster" => crate::remaster::build(disc, &stage)?,
            "radio" => fh1_radio::install::build(disc, &stage)?,
            "ui" => fh1_ui::install::build(disc, &stage)?,
            "audio" => crate::audio::build(disc, &stage)?,
            "fmv" => crate::fmv::build(disc, &stage)?,
            "story" => crate::story::build(disc, &stage)?,
            "missions" => crate::missions::build(disc, &stage)?,
            _ => unreachable!(),
        }
        // Move the old group aside before deleting it: a running engine can hold files open, and
        // deleting first would leave the group empty if the swap then fails.
        let old = private.join(format!("{group}.old"));
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        if out.exists() {
            std::fs::rename(&out, &old).with_context(|| format!("{} is in use (close fh1-engine and retry)", out.display()))?;
        }
        std::fs::rename(&stage, &out)?;
        if old.exists() {
            if let Err(e) = std::fs::remove_dir_all(&old) {
                println!("[{group}] could not remove {}: {e} (removed on the next run)", old.display());
            }
        }
        inst.pipelines.insert(group.to_owned(), hash);
        // Record progress after every group so an interrupted run resumes.
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&inst)?)?;
    }
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&inst)?)?;
    println!("[progress] {0}/{0} done", selected.len());
    println!("installation {id} ready in {}", private.display());
    Ok(())
}

/// Mirrors the `.wav` files under `from` into `to` with hard links (copies when linking fails, e.g. across volumes).
fn seed_wavs(from: &Path, to: &Path) -> Result<()> {
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            seed_wavs(&src, &dst)?;
        } else if src.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
            std::fs::create_dir_all(to)?;
            if std::fs::hard_link(&src, &dst).is_err() {
                std::fs::copy(&src, &dst)?;
            }
        }
    }
    Ok(())
}
