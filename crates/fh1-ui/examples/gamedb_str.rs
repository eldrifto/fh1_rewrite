//! gamedb_str <disc_root> <LANG> <n>... — resolves gamedb `_&n` strings; when it cannot (FM2), lists every table that has
//! the low 16 bits as a text id.

use fh1_ui::strtable::StringTables;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = StringTables::load_language(&a[1], &a[2]).expect("load");
    for n in &a[3..] {
        let n: u32 = n.parse().unwrap();
        println!("{n}: file hash {} id {} -> {:?}", n >> 16, n & 0xffff, t.gamedb(n));
        for (stem, table) in t.files.values() {
            if let Some(s) = table.texts.get(&(n as u16)) {
                println!("   {stem}: id {} = {s:?}", n & 0xffff);
            }
        }
    }
}
