# Transcode pipeline benchmark

Measured on the full `convert_to_aiff` pipeline:
fingerprint(src) → decode + write AIFF → fingerprint(dst) → compare.

## Results (release build, 3-minute stereo sine wave)

| Source | Fingerprint (1x) | Full pipeline |
|---|---|---|
| FLAC 16-bit 44.1 kHz | 163 ms | 389 ms |
| FLAC 24-bit 44.1 kHz | 174 ms | 498 ms |

~70% of pipeline time is fingerprinting (run twice). The actual decode + AIFF write is ~120–150 ms for a 3-minute track.

## Running the benchmark

```sh
cargo run --release --example bench_pipeline
```

This requires test fixtures in `tests/fixtures/`. Generate them with:

```sh
cargo test --test transcode -- --ignored generate_bench_fixtures
```
