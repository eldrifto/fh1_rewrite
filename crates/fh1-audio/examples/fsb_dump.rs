//! `fsb_dump <bank.fsb> [out_dir]`: list an FSB3 / FSB4 bank's samples and, with an output directory, write each as an XMA2 RIFF `.wav`
//! (decode with `ffmpeg -i x.wav out.wav`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let buf = std::fs::read(&a[1]).unwrap();
    let samples = fh1_audio::fsb::parse(&buf).unwrap();
    println!("{} samples", samples.len());
    for (i, s) in samples.iter().enumerate() {
        println!("{i:3} {:<24} frames {:>8} rate {} ch {} mode {:#010x} loop {}..{} bytes {}", s.name, s.frames, s.rate, s.channels, s.mode, s.loop_start, s.loop_end, s.data.len());
        if let Some(d) = a.get(2) {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(format!("{d}/{i:03}.wav"), fh1_audio::fsb::xma2_riff(&buf, s)).unwrap();
        }
    }
}
