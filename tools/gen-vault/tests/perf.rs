//! Производительность генератора: 10 000 заметок (план 0.1, задача 1.6).
//! Отдельный шаг CI: cargo test -p gen-vault --release -- --ignored
//!
//! Замер — по лучшей из трёх попыток (задача 3.2). Время на раннерах CI
//! (259 заданий Windows и 266 Linux, 24–27.10.2026):
//! - Windows: медиана 5,7 с, 90-й процентиль 9,0 с, но отдельные попытки —
//!   13–58 с (NTFS и антивирус на общей машине). Медленная попытка — разовая:
//!   прогон командной строки сразу после неё в том же задании шёл 4–8 с, ни в
//!   одном задании обе попытки не превысили 10 с (худшая лучшая — 9,2 с).
//! - Linux: медиана 0,66 с, 99-й процентиль 1,6 с, разово 3,1 с.
//!
//! Пределы ловят настоящий регресс генератора: на Linux, где время — работа
//! самого генератора, 1,5 с — это ×2,3 медианы; на Windows, где время в
//! основном уходит на файловую систему, 12 с — это ×2,1 медианы.

mod common;

use common::TempDir;
use gen_vault::{Config, generate};
use std::time::{Duration, Instant};

const ATTEMPTS: usize = 3;

fn limit() -> Duration {
    if cfg!(windows) {
        Duration::from_secs(12)
    } else {
        Duration::from_millis(1500)
    }
}

/// Делает до `attempts` замеров и останавливается на первом, что уложился в
/// `limit`. Возвращает все замеры и признак успеха.
fn best_of(
    attempts: usize,
    limit: Duration,
    mut measure: impl FnMut() -> Duration,
) -> (Vec<Duration>, bool) {
    let mut samples = Vec::with_capacity(attempts);
    for _ in 0..attempts {
        let elapsed = measure();
        samples.push(elapsed);
        if elapsed < limit {
            return (samples, true);
        }
    }
    (samples, false)
}

#[test]
fn best_of_stops_at_first_attempt_under_limit() {
    let mut times = [20, 5, 1].into_iter().map(Duration::from_secs);
    let (samples, ok) = best_of(3, Duration::from_secs(10), || times.next().unwrap());
    assert!(ok);
    assert_eq!(samples, [Duration::from_secs(20), Duration::from_secs(5)]);
}

#[test]
fn best_of_fails_when_every_attempt_is_slow() {
    let (samples, ok) = best_of(3, Duration::from_secs(10), || Duration::from_secs(11));
    assert!(!ok);
    assert_eq!(samples.len(), 3);
}

#[test]
#[ignore = "нагрузочный: запускается отдельно в выпускном профиле"]
fn ten_thousand_notes_best_of_three_under_limit() {
    let limit = limit();
    let (samples, ok) = best_of(ATTEMPTS, limit, || {
        let dir = TempDir::new("perf");
        let started = Instant::now();
        let manifest = generate(dir.path(), &Config::new(10_000, 1)).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(manifest.notes, 10_000);
        elapsed
    });
    let shown: Vec<String> = samples
        .iter()
        .map(|d| format!("{:.2} с", d.as_secs_f64()))
        .collect();
    println!(
        "gen-vault: 10 000 заметок, попытки: {} (предел {:.1} с по лучшей из {ATTEMPTS})",
        shown.join(", "),
        limit.as_secs_f64()
    );
    assert!(
        ok,
        "медленно во всех попытках: {} (предел {:.1} с)",
        shown.join(", "),
        limit.as_secs_f64()
    );
}
