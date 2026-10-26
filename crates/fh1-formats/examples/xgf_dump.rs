//! Prints an FM2 UI scene (`.xgf`) as an indented tree: `cargo run -p fh1-formats --example xgf_dump -- <UI.zip> <entry> [tag=value filter]`
//! e.g. `... -- data/work_fm2/disc/Media/UI.zip Scenes/I_Scenes/I_HUD/I_HUD_All_Divided_PartOne.xgf`
use fh1_formats::{xgf::Xgf, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let want = a[2].to_ascii_lowercase();
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase() == want).expect("entry not found").clone();
    let x = Xgf::parse(&ar.read(&e).unwrap()).unwrap();
    eprintln!("{} nodes (header {}), header attrs {}", x.nodes.len(), x.node_count, x.attr_count);
    x.walk(x.root(), &mut |i, d| {
        let n = &x.nodes[i];
        let attrs: Vec<String> = n.attrs.iter().map(|(k, v)| format!("{k}=\"{}\"", v.replace('\n', " ").chars().take(60).collect::<String>())).collect();
        println!("{}{i} <{} {}>", "  ".repeat(d), n.tag, attrs.join(" "));
    });
}
