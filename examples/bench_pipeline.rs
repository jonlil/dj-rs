use std::path::Path;
use std::time::Instant;

fn bench(label: &str, src: &Path) {
    let tmp = tempfile::tempdir().unwrap();

    let t0 = Instant::now();
    let _fp = dj_rs::transcode::fingerprint(src).unwrap();
    let t_fp = t0.elapsed();

    let t1 = Instant::now();
    let _result = dj_rs::transcode::convert_to_aiff(src, tmp.path()).unwrap();
    let t_pipeline = t1.elapsed();

    eprintln!("{label}");
    eprintln!("  fingerprint (1x):  {t_fp:?}");
    eprintln!("  full pipeline:     {t_pipeline:?}");
}

fn main() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));

    let files = [
        ("FLAC 16-bit 3min", "bench_3min.flac"),
        ("FLAC 24-bit 3min", "bench_3min_24bit.flac"),
    ];

    for (label, name) in files {
        let path = dir.join(name);
        if !path.exists() {
            eprintln!("Missing {name} — run: cargo test --test transcode -- --ignored generate_bench_fixtures");
            continue;
        }
        bench(label, &path);
    }
}
