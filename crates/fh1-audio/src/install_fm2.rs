//! Forza Motorsport 2 car engine audio -> the FH1 car audio model (`cars/FM2_<MEDIA>.json` + decoded `banks/`).
//!
//! FM2 differs from FH1 in layout, not in kind:
//! - `Engines/DSPTuning.zip`: `<MEDIA>_DSPTuning.xml` (`<Car audio_rpm_min audio_rpm_max>`, `GlobalCarVolume`, and `Exhaust/Upgrade level="0"` with the
//!   stock harmonic tuning file, volumes, exhaust DSP and `Engine/Upgrade level="0"/EngineA|EngineB` DSP).
//! - `Engines/HarmonicTuning.zip`: `<Soundbank name=BANK>` with the loop sets `EngineA`, `EngineB`, `ExhaustL`, `ExhaustR` (the same `Loop` rows as FH1).
//! - `Engines/Soundbanks/LOD1/<BANK>.fsb`: FSB3, XMA (see [`crate::fsb`]).
//!
//! Mapping (INFERRED from the names and from the FH1 model, whose intake / ambient / exhaust emitters play the same kind of loop sets): `EngineA` ->
//! the intake emitter, `EngineB` -> the ambient emitter, `ExhaustL` + `ExhaustR` -> the exhaust emitter, each with its own DSP from the DSP tuning.
//! Transmission / turbo / supercharger volumes come from the Exhaust upgrade's attributes. Views (camera mixes) stay at the defaults.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};

use crate::install::{bank_stem, find_ci, read_zip, BankInfo, SampleInfo};
use crate::tuning::{CarAudio, ClipRef, Curve, Dsp, El, Emitter, EngineExtras, SharedSounds, TyreRefs, ViewMix};
use crate::{fsb, wav, xma};

/// Prefix of the converted car audio names (`cars/FM2_<MEDIA>.json`), what a car's `sound_donor` names.
pub const PREFIX: &str = "FM2_";

/// Stem of a converted shared bank (`FM2_Tires` ...): the FH1 install already owns `Tires`, `Wind`, `Transmissions` and `Turbos`.
fn shared_stem(name: &str) -> String {
    format!("{PREFIX}{name}")
}

/// The shared samples every FM2 car uses (one fixed pick per synth slot; FM2 chooses by FMOD event, which is not read, so these are
/// representatives by name: asphalt / concrete / dirt / grass tyre loops, the wind, a mid transmission whine, turbo loop, shift clicks and blow-off).
pub struct CarInfo {
    /// Level (0..3) of the car's stock transmission row (`List_UpgradeTransmission`, `IsStock = 1`).
    pub trans_level: i64,
    /// Level (1..3) of the stock turbo row of the car's engine, when it has one.
    pub turbo_level: Option<i64>,
}

/// The shared samples for one car. THE LINK (read from the FEV event names): FM2's car FEVs name their events by upgrade LEVEL, not by car:
/// `Transmissions` has `GearShift` / `ShiftStart` / `ShiftComplete` and the whine event under `L0`..`L3`, `Turbos` has `L1`..`L3`, `SuperChargers` has `CSC` / `DSC`
/// groups with `L1`..`L3`. The game plays the event of the fitted part's level, so a car's stock sound follows its stock part rows:
/// - transmission level 0 / 1: `NewShifts` (the `SFT_truck_*` clips) and the `Trans_*` whine; level 2: `DogboxShift` and the `gear*Hz` whine; level 3: `PorscheDogshift`
///   and the `gear*Hz` whine (the whine events of L0 / L1 and of L2 / L3 are identical in the FEV);
/// - turbo level 1: `turbo1_loop*` + `POR_944_Blowoff1`; level 2: `turbo2mod_loop*` + `Fer_F40_Blowoff1`; level 3: `turbo2mod_loop*` + `F40Edit_L3_bv1`.
/// Which `wavebankindex` loop plays at which RPM is the event's own parameter envelope, which the synth replaces with its single loop per slot (INFERRED pick).
fn shared_sounds_for(info: Option<&CarInfo>) -> SharedSounds {
    let mut sh = shared_sounds();
    let c = |bank: &str, name: &str| ClipRef { bank: shared_stem(bank), name: name.to_owned() };
    let Some(i) = info else { return sh };
    let (shift, whine): (&[&str], &str) = match i.trans_level {
        2 => (&["dogbox1", "dogbox2"], "gear1_50Hz"),
        3 => (&["PorscheDogshift1", "PorscheDogshift4", "PorscheDogshift7", "PorscheDogshift8"], "gear1_50Hz"),
        _ => (&["SFT_truck_00", "SFT_truck_01", "SFT_truck_02", "SFT_truck_04", "SFT_truck_05"], "Trans_01500"),
    };
    sh.shift = shift.iter().map(|n| c("Transmissions", n)).collect();
    sh.whine = Some(c("Transmissions", whine));
    match i.turbo_level {
        Some(1) => {
            sh.turbo = Some(c("Turbos", "turbo1_loop7"));
            sh.blowoff = Some(c("Turbos", "POR_944_Blowoff1"));
            sh.blowoff_med = sh.blowoff.clone();
        }
        Some(2) => {
            sh.turbo = Some(c("Turbos", "turbo2mod_loop7"));
            sh.blowoff = Some(c("Turbos", "Fer_F40_Blowoff1"));
            sh.blowoff_med = sh.blowoff.clone();
        }
        Some(_) => {
            sh.turbo = Some(c("Turbos", "turbo2mod_loop7"));
            sh.blowoff = Some(c("Turbos", "F40Edit_L3_bv1"));
            sh.blowoff_med = sh.blowoff.clone();
        }
        None => {}
    }
    sh
}

fn shared_sounds() -> SharedSounds {
    let c = |bank: &str, name: &str| ClipRef { bank: shared_stem(bank), name: name.to_owned() };
    let ty = |group: &str, roll: &str, pre: &str, skid: &str, high: &str| TyreRefs {
        group: group.to_owned(),
        roll: c("Tires", roll),
        pre_skid: c("Tires", pre),
        skid: c("Tires", skid),
        skid_high: c("Tires", high),
    };
    SharedSounds {
        tyres: vec![
            ty("OnRoad", "ASP_RoadNoiseSlow_00", "CON_HDF_Preskid_00", "ASP_HighDownForce_Skid26", "ASP_HighDownForce_SkidHigh14"),
            ty("Trackway", "ASP_RoadNoiseFast_00", "CON_LDF_Preskid_00", "ASP_HighDownForce_Skid25", "ASP_HighDownForce_SkidHigh15"),
            ty("Brick", "ASP_RoadNoiseSlow_01", "CON_HDF_Preskid_01", "ASP_HighDownForce_Skid24", "ASP_HighDownForce_SkidHigh16"),
            ty("OffRoad", "DRT_RoadnoiseFast_01", "DRT_Medloss_00", "DRT_Fullloss_00", "DRT_RoadnoiseFast_02"),
            ty("Grass", "GRS_RoadnoiseFast_00", "GRS_Medskid_00", "GRS_Fullskid_00", "GRS_RoadnoiseFast_01"),
        ],
        offroad_chassis: Some(c("Tires", "Wheelwell_000")),
        wind: Some(c("Wind", "ViolentCarWind")),
        whine: Some(c("Transmissions", "Trans_01500")),
        turbo: Some(c("Turbos", "turbo1_loop7")),
        shift: ["Fer_shiftEnd1", "Fer_shiftEnd2", "Fer_shiftEnd3", "Fer_shiftEnd4"].iter().map(|n| c("Transmissions", n)).collect(),
        blowoff: Some(c("Turbos", "POR_944_Blowoff1")),
        blowoff_med: Some(c("Turbos", "Fer_F40_Blowoff1")),
    }
}

/// FM2 sample names keep the file extension (`Trans_01500.wav`, truncated to `...SkidHigh14.w` at 30 bytes): the converted bank uses the bare name.
fn clean_name(n: &str) -> String {
    n.strip_suffix(".wav").or_else(|| n.strip_suffix(".w")).unwrap_or(n).to_owned()
}

/// The camera mixes of `<MEDIA>_ED.xml` (`EmitterData.zip`): four `CarListener` blocks per car, each with `EmissionGroupVolumes` (engine A / B, exhaust L / R)
/// and `OtherVolumes` (tyre, transmission, turbo, supercharger, wind ...). INFERRED assignment to the synth's views: the block order is cockpit (listener at the
/// driver, z ~ +0.4..0.9), chase (listener 3.3 m behind the car), hood / bumper (near the centre) and the default (the origin); the volumes agree with FH1's
/// mixes for those views (cockpit: tyre 0.8, transmission 0.7, wind 0.69; default: wind 0.2). EngineA -> intake, EngineB -> ambient.
fn views(ed: &El) -> BTreeMap<String, ViewMix> {
    let names = ["Cockpit", "Follow", "Front", "Default"];
    let mut out = BTreeMap::new();
    for (name, l) in names.iter().zip(ed.kids.iter().filter(|k| k.name == "CarListener")) {
        let (Some(eg), Some(o)) = (kid(l, "EmissionGroupVolumes"), kid(l, "OtherVolumes")) else { continue };
        let d = ViewMix::default();
        out.insert(
            (*name).to_owned(),
            ViewMix {
                intake: eg.f("EngineA").unwrap_or(d.intake),
                ambient: eg.f("EngineB").unwrap_or(d.ambient),
                exhaust_l: eg.f("ExhaustL").unwrap_or(d.exhaust_l),
                exhaust_r: eg.f("ExhaustR").unwrap_or(d.exhaust_r),
                tire: o.f("TireVolume").unwrap_or(d.tire),
                transmission: o.f("TransmissionVolume").unwrap_or(d.transmission),
                turbo: o.f("TurboVolume").unwrap_or(d.turbo),
                supercharger: o.f("SuperChargerVolume").unwrap_or(d.supercharger),
                wind: o.f("CarWindVolume").unwrap_or(d.wind),
            },
        );
    }
    out
}

/// The cylinder count from an FM2 bank name (`ACU_NSXStock_91_092305_6A0`: the last `_` field starts with it).
fn cylinders(bank: &str) -> u32 {
    bank.rsplit('_').next().and_then(|t| t.chars().next()).and_then(|c| c.to_digit(10)).filter(|c| *c >= 1).unwrap_or(4)
}

fn kid<'a>(e: &'a El, name: &str) -> Option<&'a El> {
    e.kids.iter().find(|k| k.name == name)
}

/// The stock `Upgrade` child of `e`: level 0 when there is one, else the first (some cars only carry a single, higher level).
fn stock(e: &El) -> Option<&El> {
    e.kids.iter().find(|k| k.name == "Upgrade" && k.attr("level") == Some("0")).or_else(|| e.kids.iter().find(|k| k.name == "Upgrade"))
}

fn emitter(ht: &El, tuning: &str, sets: &[&str], dsp: Dsp) -> Option<Emitter> {
    let mut em = Emitter::parse(tuning, ht)?;
    em.sets.retain(|k, _| sets.contains(&k.as_str()));
    if em.sets.is_empty() {
        return None;
    }
    em.bank = format!("{}.fsb", em.bank.trim_end_matches(".fsb"));
    em.dsp = dsp;
    Some(em)
}

/// Converts the cars in `media_names` that have FM2 tuning; returns media name -> audio name (`FM2_<MEDIA>`). Writes `out/cars`, `out/banks`.
pub fn build(disc: &Path, out: &Path, media_names: &[String], infos: &BTreeMap<String, CarInfo>) -> Result<BTreeMap<String, String>> {
    let ffmpeg = xma::ffmpeg().context("audio needs ffmpeg for XMA decoding (PATH or FH1_FFMPEG)")?;
    let engines = fh1_formats::path::resolve(&disc.join("media/audio/cars/Engines"));
    let dsp_zip = read_zip(&find_ci(&engines, "DSPTuning.zip").context("DSPTuning.zip")?)?;
    let ht_zip = read_zip(&find_ci(&engines, "HarmonicTuning.zip").context("HarmonicTuning.zip")?)?;
    let ed_zip = find_ci(&engines.parent().context("cars dir")?.to_path_buf(), "EmitterData.zip").map(|p| read_zip(&p)).transpose()?.unwrap_or_default();
    let lod1 = fh1_formats::path::resolve(&engines.join("Soundbanks/LOD1"));
    std::fs::create_dir_all(out.join("cars"))?;

    let mut done = BTreeMap::new();
    let mut banks: BTreeSet<String> = BTreeSet::new();
    let mut skipped = Vec::new();
    for media in media_names {
        let Some(xml) = dsp_zip.get(&format!("{}_dsptuning.xml", media.to_ascii_lowercase())) else {
            skipped.push(format!("{media}: no DSP tuning"));
            continue;
        };
        let r = (|| -> Result<CarAudio> {
            let root = El::parse(&String::from_utf8_lossy(xml)).context("DSP tuning xml")?;
            let exhaust = kid(&root, "Exhaust").context("no Exhaust")?;
            let up = stock(exhaust).context("no stock exhaust upgrade")?;
            let tuning = up.attr("Tuning").context("no Tuning")?;
            let ht_xml = ht_zip.get(&tuning.to_ascii_lowercase()).with_context(|| format!("harmonic tuning {tuning} missing"))?;
            let ht = El::parse(&String::from_utf8_lossy(ht_xml)).with_context(|| tuning.to_owned())?;
            let eng = kid(up, "Engine").and_then(stock);
            let dsp_of = |name: &str| eng.and_then(|e| kid(e, name)).map(Dsp::parse).unwrap_or_default();
            let intake = emitter(&ht, tuning, &["EngineA"], dsp_of("EngineA"));
            let ambient = emitter(&ht, tuning, &["EngineB"], dsp_of("EngineB"));
            let exh = emitter(&ht, tuning, &["ExhaustL", "ExhaustR"], Dsp::parse(up));
            let bank = ht.attr("name").unwrap_or("");
            let f = |e: &El, k: &str, d: f32| e.f(k).unwrap_or(d);
            let trans = f(up, "TransmissionVolume", 0.4);
            Ok(CarAudio {
                car: media.clone(),
                rpm_idle: root.f("audio_rpm_min").or_else(|| root.f("audio_rpm_idle")).unwrap_or(800.0),
                rpm_redline: root.f("audio_rpm_max").or_else(|| root.f("audio_rpm_redline")).unwrap_or(7000.0),
                cylinders: cylinders(bank),
                engine_volume: f(up, "VolumeAdjustment", 0.8),
                transmission_volume: trans,
                shift_volume: trans,
                turbo_volume: f(up, "TurboVolume", 0.3),
                supercharger_volume: f(up, "SuperChargerVolume", 0.2),
                global_volume: kid(&root, "GlobalCarVolume").and_then(Curve::parse),
                intake,
                ambient,
                exhaust: exh,
                views: ed_zip
                    .get(&format!("{}_ed.xml", media.to_ascii_lowercase()))
                    .and_then(|x| El::parse(&String::from_utf8_lossy(x)).ok())
                    .map(|e| views(&e))
                    .unwrap_or_default(),
                extras: EngineExtras::default(),
                shared: Some(shared_sounds_for(infos.get(media))),
            })
        })();
        match r {
            Ok(a) => {
                banks.extend(a.banks().map(str::to_owned));
                let name = format!("{PREFIX}{media}");
                std::fs::write(out.join("cars").join(format!("{name}.json")), serde_json::to_vec_pretty(&a)?)?;
                done.insert(media.clone(), name);
            }
            Err(e) => skipped.push(format!("{media}: {e:#}")),
        }
    }
    println!("[fm2 audio] {} cars converted, {} skipped, {} engine banks", done.len(), skipped.len(), banks.len());
    for s in skipped.iter().take(8) {
        println!("[fm2 audio]   skipped {s}");
    }

    // Decode the banks the cars use (one ffmpeg process per sample, in parallel; samples already decoded are kept).
    let mut to_decode: Vec<(String, std::path::PathBuf)> = Vec::new();
    for b in &banks {
        match find_ci(&lod1, b) {
            Some(p) => to_decode.push((bank_stem(b).to_owned(), p)),
            None => println!("[fm2 audio]   bank {b} not on disc"),
        }
    }
    let car_banks = fh1_formats::path::resolve(&disc.join("media/audio/cars"));
    for (dir, name) in [("Tires", "Tires"), ("Wind", "Wind"), ("Transmissions", "Transmissions"), ("Turbos", "Turbos")] {
        match find_ci(&car_banks, dir).and_then(|d| find_ci(&d, &format!("{name}.fsb"))) {
            Some(p) => to_decode.push((shared_stem(name), p)),
            None => println!("[fm2 audio]   shared bank {name} not on disc"),
        }
    }
    let mut jobs: Vec<(Arc<Vec<u8>>, String, usize)> = Vec::new();
    for (stem, path) in &to_decode {
        let buf = Arc::new(std::fs::read(path).with_context(|| path.display().to_string())?);
        let samples = fsb::parse(&buf).with_context(|| stem.clone())?;
        let stem = stem.clone();
        std::fs::create_dir_all(out.join("banks").join(&stem))?;
        let info = BankInfo {
            samples: samples
                .iter()
                .map(|s| SampleInfo { name: clean_name(&s.name), rate: s.rate, channels: s.channels, frames: s.frames, loop_start: s.loop_start, loop_end: s.loop_end })
                .collect(),
        };
        std::fs::write(out.join("banks").join(format!("{stem}.json")), serde_json::to_vec_pretty(&info)?)?;
        jobs.extend((0..samples.len()).map(|i| (buf.clone(), stem.clone(), i)));
    }
    let next = AtomicUsize::new(0);
    let errors = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((buf, stem, idx)) = jobs.get(i) else { break };
                let r = (|| -> Result<()> {
                    let s = &fsb::parse(buf)?[*idx];
                    let wav_path = out.join("banks").join(stem).join(format!("{idx:03}.wav"));
                    if std::fs::metadata(&wav_path).is_ok_and(|m| m.len() > 44) {
                        return Ok(());
                    }
                    if s.mode & fsb::MODE_XMA == 0 {
                        bail!("mode {:#x} is not XMA", s.mode);
                    }
                    let pcm = xma::decode(&ffmpeg, fsb::xma2_riff(buf, s), s.channels, s.frames)?;
                    wav::write(&wav_path, s.rate, s.channels, &pcm)
                })();
                if let Err(e) = r {
                    errors.lock().unwrap().push(format!("{stem}#{idx}: {e:#}"));
                }
                if i % 1000 == 0 {
                    println!("[fm2 audio]   decoded {i}/{}", jobs.len());
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap();
    println!("[fm2 audio] {} samples, {} failed", jobs.len(), errors.len());
    for e in errors.iter().take(8) {
        println!("[fm2 audio]   {e}");
    }
    Ok(done)
}
