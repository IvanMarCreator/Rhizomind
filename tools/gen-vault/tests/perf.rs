//! Производительность: 10 000 заметок быстрее 10 секунд (план 0.1, задача 1.6).
//! Отдельный шаг CI: cargo test -p gen-vault --release -- --ignored

mod common;

use common::TempDir;
use gen_vault::{Config, generate};
use std::time::{Duration, Instant};

#[test]
#[ignore = "нагрузочный: запускается отдельно в выпускном профиле"]
fn ten_thousand_notes_in_under_ten_seconds() {
    let dir = TempDir::new("perf");
    let started = Instant::now();
    let manifest = generate(dir.path(), &Config::new(10_000, 1)).unwrap();
    let elapsed = started.elapsed();
    println!(
        "gen-vault: 10 000 заметок за {:.2} с",
        elapsed.as_secs_f64()
    );
    assert_eq!(manifest.notes, 10_000);
    assert!(elapsed < Duration::from_secs(10), "медленно: {elapsed:?}");
}
