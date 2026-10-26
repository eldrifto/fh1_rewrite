//! Checks the `.xgf` decode against FM2's own HUD scenes. Skipped when the FM2 disc copy is not on this machine.
use fh1_formats::{xgf::Xgf, zip::Archive};

fn ui_zip() -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/work_fm2/disc/Media/UI.zip");
    p.exists().then_some(p)
}

#[test]
fn hud_scenes_decode_completely() {
    let Some(p) = ui_zip() else { return };
    let mut ar = Archive::open(&p).unwrap();
    for name in ["Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_Common.xgf", "Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_PartOne.xgf", "Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_PartTwo.xgf"] {
        let e = ar.entries.iter().find(|e| e.name == name).unwrap().clone();
        let x = Xgf::parse(&ar.read(&e).unwrap()).unwrap();
        assert_eq!(x.nodes.len(), x.node_count, "{name}");
        // Every node is reachable from the root (the tree links are consistent).
        let mut seen = 0;
        x.walk(x.root(), &mut |_, _| seen += 1);
        assert_eq!(seen, x.nodes.len(), "{name}: unreachable nodes");
        // Every attribute resolved to a string (an unresolved offset decodes to "" for both name and value).
        let empty_names = x.nodes.iter().flat_map(|n| &n.attrs).filter(|(k, _)| k.is_empty()).count();
        assert_eq!(empty_names, 0, "{name}: attribute names that did not resolve");
        let total_attrs: usize = x.nodes.iter().map(|n| n.attrs.len()).sum();
        eprintln!("{name}: {} nodes, {} attributes read, header says {}", x.nodes.len(), total_attrs, x.attr_count);
    }
}

#[test]
fn hud_meshes_decode() {
    let Some(p) = ui_zip() else { return };
    let mut ar = Archive::open(&p).unwrap();
    for (name, n_mesh) in [("Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_PartOne.xgf", 6), ("Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_PartTwo.xgf", 52)] {
        let e = ar.entries.iter().find(|e| e.name == name).unwrap().clone();
        let x = Xgf::parse(&ar.read(&e).unwrap()).unwrap();
        assert_eq!(x.meshes.len(), n_mesh, "{name}");
        // The rectangle every scene starts with: +-50 units, three by three vertices per side, indices in range.
        let rect = x.nodes.iter().find(|n| n.attr("type") == Some("modelbuffer") && n.attr("name") == Some("Rectangle"));
        if let Some(rect) = rect {
            let r = &x.meshes[rect.attr("modeldata").unwrap().parse::<usize>().unwrap()];
            assert_eq!((r.bbox_min[0], r.bbox_max[0]), (-50.0, 50.0), "{name}");
            assert_eq!(r.vertices.len(), 18, "{name}");
        }
        for m in &x.meshes {
            assert!(m.groups.iter().flatten().all(|&i| (i as usize) < m.vertices.len()), "{name}: index out of range");
        }
        // Every modelbuffer's modeldata points at a mesh.
        for n in x.nodes.iter().filter(|n| n.tag == "node" && n.attr("type") == Some("modelbuffer")) {
            let i: usize = n.attr("modeldata").unwrap().parse().unwrap();
            assert!(i < x.meshes.len(), "{name}: modeldata {i}");
        }
        eprintln!("{name}: {} meshes, {} vertices", x.meshes.len(), x.meshes.iter().map(|m| m.vertices.len()).sum::<usize>());
    }
}
