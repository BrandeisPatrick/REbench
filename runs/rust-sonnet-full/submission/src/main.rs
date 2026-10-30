mod fmt;
mod parse;
mod eval;
mod load;

use std::io::Write;

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().collect();
    let mut inp = String::new();
    let mut out = String::new();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--in" => { inp = args[i + 1].clone(); i += 2; }
            "--out" => { out = args[i + 1].clone(); i += 2; }
            _ => i += 1,
        }
    }
    let h = std::thread::Builder::new().stack_size(1usize << 30).spawn(move || {
        let t0 = std::time::Instant::now();
        let wb = load::load(&inp).expect("load");
        if std::env::var("CALC_TIME").is_ok() { eprintln!("load {:?}", t0.elapsed()); }
        let json = eval::run(wb);
        if std::env::var("CALC_TIME").is_ok() { eprintln!("total {:?}", t0.elapsed()); }
        let mut f = std::fs::File::create(&out).expect("create");
        f.write_all(json.as_bytes()).unwrap();
    }).unwrap();
    h.join().unwrap();
}
