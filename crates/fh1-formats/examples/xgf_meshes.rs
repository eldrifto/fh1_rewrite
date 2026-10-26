//! Lists the meshes of an FM2 UI scene by the model buffers that name them: `cargo run -p fh1-formats --example xgf_meshes -- <UI.zip> <entry> [name filter]`
use fh1_formats::{xgf::Xgf, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let want = a[2].to_ascii_lowercase();
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase() == want).expect("entry not found").clone();
    let x = Xgf::parse(&ar.read(&e).unwrap()).unwrap();
    let filter = a.get(3).map(|s| s.to_ascii_lowercase());
    for (i, n) in x.nodes.iter().enumerate().filter(|(_, n)| n.attr("type") == Some("modelbuffer")) {
        let name = n.attr("name").unwrap_or("");
        if filter.as_ref().is_some_and(|f| !name.to_ascii_lowercase().contains(f)) {
            continue;
        }
        let m = &x.meshes[n.attr("modeldata").unwrap().parse::<usize>().unwrap()];
        println!("node {i} {name:?} xmlid {:?} mesh {:?}: {} verts, groups {:?} bbox {:.2?}..{:.2?}", n.attr("xmlid"), n.attr("modeldata"), m.vertices.len(), m.groups.iter().map(Vec::len).collect::<Vec<_>>(), m.bbox_min, m.bbox_max);
    }
}
