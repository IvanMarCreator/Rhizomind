//! Замеры ядра (задача 0.2-7.1, С2-8.3): индекс на хранилище генератора.
//!
//! Запуск (шаг CI «Замеры ядра», задание `perf-core`):
//! `cargo test -p rhizomind --release --lib -- --ignored --nocapture --test-threads=1 perf_`.
//!
//! Методика — как `tests/perf/stats.ts` (решения 103, 104): каждый замер
//! повторяется [`REPEATS`] раз, вывод — по медиане повторов, худший повтор —
//! рядом. Пределы проверяются только в выпускной сборке на Windows и только
//! на 10 000 заметок; на Linux и на 50 000 — цифры и отметка «превышение»
//! без падения (С2-8.3: «на Linux — справочно», «на 50 000 — замерить»).
//!
//! Размер хранилища: 10 000 заметок; `RHIZOMIND_PERF_50K=1` добавляет
//! 50 000 (задание `perf-core` по запуску вручную), `RHIZOMIND_PERF_NOTES=N`
//! заменяет оба одним размером (проверка тестов на маленьком хранилище в
//! отладочной сборке).
//!
//! Цифры уходят в вывод теста и строкой `::notice` (`::error` при
//! нарушенном пределе) — команда GitHub Actions: она становится пометкой
//! прогона, которую видно без входа в GitHub (API пометок проверки).
//!
//! Здесь — общие помощники замеров ([`Report`], [`memory`], [`scales`]) и
//! замеры индекса: сборка с нуля, открытие с готовым `index.json` (чтение и
//! сверка), размер `index.json`, память ядра сверх 0.1. Запросы окна
//! (`backlinks_list` холодный и повторный, `tags_list`, `tag_notes`) и
//! `NoteList::replace_notes` — `commands::note_list_perf`; план
//! переписывания ссылок — `relink::perf_tests` (каркас, второй прогон).

use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::store::{self, IndexStore, Load, WriteOutcome};
use super::{VaultIndex, rebuild};
use crate::commands::NoteList;
use crate::vault::Vault;

/// Повторов каждого замера (медиана — по ним).
pub(crate) const REPEATS: usize = 5;

/// Размер, на котором пределы С2-8.3 проверяются.
pub(crate) const CHECKED_NOTES: usize = 10_000;

/// Размеры хранилища для этого прогона (см. заголовок модуля).
pub(crate) fn scales() -> Vec<usize> {
    if let Some(n) = std::env::var("RHIZOMIND_PERF_NOTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return vec![n];
    }
    let mut scales = vec![10_000];
    if std::env::var("RHIZOMIND_PERF_50K").is_ok_and(|v| v == "1") {
        scales.push(50_000);
    }
    scales
}

/// Проверяются ли пределы: выпускная сборка на Windows (С2-8.3).
pub(crate) fn enforced() -> bool {
    cfg!(windows) && !cfg!(debug_assertions)
}

/// Сборка — для подписи цифр.
pub(crate) fn build_kind() -> &'static str {
    if cfg!(debug_assertions) {
        "отладочная"
    } else {
        "выпускная"
    }
}

/// Медиана (как `median` в `tests/perf/stats.ts`).
pub(crate) fn median(values: &[f64]) -> f64 {
    assert!(!values.is_empty(), "нет значений");
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    let mid = s.len() / 2;
    if s.len() % 2 == 1 {
        s[mid]
    } else {
        (s[mid - 1] + s[mid]) / 2.0
    }
}

/// Наибольшее значение.
pub(crate) fn worst(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::MIN, f64::max)
}

/// Миллисекунды.
pub(crate) fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Время выполнения `f` в мс и его результат.
pub(crate) fn timed<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let start = Instant::now();
    let value = f();
    (ms(start.elapsed()), value)
}

/// Сводка замеров одного теста: строки «что — медиана — худший — предел —
/// итог», печать, пометка GitHub Actions и падение при нарушенном пределе.
pub(crate) struct Report {
    title: String,
    lines: Vec<String>,
    violations: Vec<String>,
}

impl Report {
    pub(crate) fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            lines: vec![format!(
                "{title}: сборка {}, {}, потоков {}, повторов {REPEATS}",
                build_kind(),
                std::env::consts::OS,
                std::thread::available_parallelism().map_or(1, |n| n.get()),
            )],
            violations: Vec::new(),
        }
    }

    /// Справочная строка.
    pub(crate) fn note(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Замер по повторам `values` (`unit` — подпись) против предела `limit`
    /// (по медиане). `checked` — предел проверяется (10 000 и [`enforced`]);
    /// иначе превышение только отмечается.
    pub(crate) fn measure(
        &mut self,
        what: &str,
        values: &[f64],
        unit: &str,
        limit: Option<f64>,
        checked: bool,
    ) {
        let med = median(values);
        let bad = worst(values);
        let all = values
            .iter()
            .map(|v| format!("{v:.1}"))
            .collect::<Vec<_>>()
            .join(" ");
        let verdict = match limit {
            None => "справочно".to_owned(),
            Some(limit) if med <= limit => format!("ok (≤ {limit} {unit})"),
            Some(limit) if checked && enforced() => {
                self.violations
                    .push(format!("{what}: медиана {med:.1} {unit} > {limit} {unit}"));
                format!("НАРУШЕНО (> {limit} {unit})")
            }
            Some(limit) => format!("превышение (> {limit} {unit}), не проверяется здесь"),
        };
        self.lines.push(format!(
            "  {what}: медиана {med:.1} {unit}, худший {bad:.1} {unit} [{all}] — {verdict}"
        ));
    }

    /// Печать, пометка GitHub Actions; нарушенный предел — падение теста с
    /// понятным сообщением.
    pub(crate) fn finish(self) {
        let text = self.lines.join("\n");
        println!("{text}");
        let level = if self.violations.is_empty() {
            "notice"
        } else {
            "error"
        };
        if std::env::var_os("GITHUB_ACTIONS").is_some() {
            println!(
                "::{level} title={}::{}",
                escape_property(&self.title),
                escape_data(&text)
            );
        }
        assert!(
            self.violations.is_empty(),
            "{}: цели С2-8.3 не выполнены:\n{}",
            self.title,
            self.violations.join("\n")
        );
    }
}

/// Экранирование текста команды GitHub Actions.
fn escape_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_property(s: &str) -> String {
    escape_data(s).replace(':', "%3A").replace(',', "%2C")
}

/// Память процесса, байт: `private` — закреплённая за процессом память
/// (Windows: `PrivateUsage`, коммит; Linux: `RssAnon`, анонимная
/// резидентная — куча), `resident` — рабочий набор (Windows
/// `WorkingSetSize`, Linux `VmRSS`). `None` — ОС не поддержана.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Memory {
    pub private: u64,
    pub resident: u64,
}

#[cfg(windows)]
pub(crate) fn memory() -> Option<Memory> {
    /// `PROCESS_MEMORY_COUNTERS_EX` (psapi.h).
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }
    // Функция kernel32 (Windows 7+): фича windows `Win32_System_ProcessStatus`
    // не подключена, а замер — только в тестах.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }
    let mut c = Counters {
        cb: u32::try_from(std::mem::size_of::<Counters>()).ok()?,
        ..Counters::default()
    };
    let cb = c.cb;
    // SAFETY: псевдодескриптор текущего процесса не требует закрытия;
    // `c` — структура нужного размера, `cb` равен её размеру.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut c, cb) };
    (ok != 0).then_some(Memory {
        private: c.private_usage as u64,
        resident: c.working_set_size as u64,
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn memory() -> Option<Memory> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| -> Option<u64> {
        let line = status.lines().find(|l| l.starts_with(name))?;
        let kb: u64 = line[name.len()..]
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        Some(kb * 1024)
    };
    Some(Memory {
        private: field("RssAnon:")?,
        resident: field("VmRSS:")?,
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(crate) fn memory() -> Option<Memory> {
    None
}

pub(crate) const MB: f64 = 1024.0 * 1024.0;

/// Пределы С2-8.3 для размера `notes`: (сборка с нуля, мс; открытие с
/// готовым индексом, мс; `index.json`, МБ; память сверх 0.1, МБ).
fn limits(notes: usize) -> (f64, f64, f64, f64) {
    if notes > CHECKED_NOTES {
        (25_000.0, 4_000.0, 75.0, 300.0)
    } else {
        (5_000.0, 1_000.0, 15.0, 60.0)
    }
}

/// Сборка с нуля, открытие с готовым `index.json`, размер `index.json`,
/// память ядра сверх 0.1 (С2-8.3).
///
/// - **Сборка** — [`rebuild`] (как фоновый поток окна): пул ядра, чтение
///   файлов параллельно; новый пустой индекс на каждый повтор.
/// - **Открытие с готовым индексом** — [`store::load`] (чтение и разбор
///   `index.json`) и [`store::reconcile`] (обход папки, сверка размера и
///   даты изменения) — то, что делает поток индекса при открытии окна.
/// - **Память сверх 0.1** — разница памяти процесса ([`memory`], закреплённая
///   за процессом) до и после открытия с готовым индексом при уже открытом
///   хранилище 0.1 (`Vault` и список заметок `NoteList`: ссылки и быстрый
///   переход). Байты файла к моменту замера освобождены — в памяти остаётся
///   то, что живёт, пока окно открыто. Открытие и память меряются в
///   отдельном процессе на каждый повтор ([`memory_child_process`]): в этом
///   процессе куча уже выросла на сборках, и распределитель отдал бы
///   освобождённое ими — замер занижался бы.
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_index_build_open_size_memory() {
    for notes in scales() {
        let checked = notes == CHECKED_NOTES;
        let (build_limit, open_limit, size_limit, memory_limit) = limits(notes);
        let mut report = Report::new(&format!("perf-core индекс, {notes} заметок"));

        let dir = tempfile::tempdir().expect("временная папка");
        let root = dir.path().join("vault");
        let (gen_ms, _) = timed(|| {
            gen_vault::generate(&root, &gen_vault::Config::new(notes, 7)).expect("генератор")
        });
        report.note(format!("  генератор: {gen_ms:.0} мс"));
        let vault = Vault::open(&root).expect("хранилище");

        // Сборка с нуля.
        let mut build = Vec::new();
        let mut built = None;
        for _ in 0..REPEATS {
            let index = RwLock::new(VaultIndex::new());
            let (took, result) = timed(|| rebuild(&vault, &index, &|_| {}));
            result.expect("собран");
            build.push(took);
            built = Some(index);
        }
        let built = built.expect("индекс");
        let total = built.read().expect("индекс").entries().count();
        assert!(total >= notes, "в индексе {total} из {notes}");
        report.note(format!("  заметок в индексе: {total}"));
        report.measure(
            "сборка индекса с нуля",
            &build,
            "мс",
            Some(build_limit),
            checked,
        );

        // Размер index.json.
        assert_eq!(
            IndexStore::new().write_now(&vault, &built),
            WriteOutcome::Written
        );
        drop(built);
        let size = std::fs::metadata(root.join(".rhizomind").join(store::FILE_NAME))
            .expect("index.json")
            .len() as f64;
        report.measure(
            &format!(
                "размер index.json ({:.0} байт на заметку)",
                size / total as f64
            ),
            &[size / MB],
            "МБ",
            Some(size_limit),
            checked,
        );

        // Открытие с готовым индексом и память сверх 0.1 — в отдельном
        // процессе на каждый повтор (этот же тестовый файл,
        // [`memory_child_process`]): куча чистая, освобождённое сборками
        // выше не подмешивается в замер.
        let mut open = Vec::new();
        let mut private = Vec::new();
        let mut resident = Vec::new();
        for _ in 0..REPEATS {
            let out = std::process::Command::new(std::env::current_exe().expect("тестовый файл"))
                .args([
                    "--exact",
                    "index::perf_tests::memory_child_process",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_VAULT, &root)
                .env_remove("GITHUB_ACTIONS")
                .output()
                .expect("дочерний процесс");
            let stdout = String::from_utf8_lossy(&out.stdout);
            let line = stdout
                .lines()
                .find_map(|l| l.find(CHILD_MARK).map(|at| &l[at + CHILD_MARK.len()..]))
                .unwrap_or_else(|| {
                    panic!(
                        "нет ответа дочернего процесса:\n{stdout}\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    )
                });
            let v: Vec<f64> = line
                .split_whitespace()
                .map(|x| x.parse().expect("число"))
                .collect();
            assert_eq!(v[3] as usize, total, "заметок после открытия");
            open.push(v[0]);
            if v[1].is_finite() {
                private.push(v[1] / MB);
                resident.push(v[2] / MB);
            }
        }
        report.measure(
            "открытие с готовым index.json (чтение и сверка)",
            &open,
            "мс",
            Some(open_limit),
            checked,
        );
        if private.is_empty() {
            report.note("  память: ОС не поддержана — не замерена");
        } else {
            report.measure(
                "память ядра сверх 0.1 (закреплённая)",
                &private,
                "МБ",
                Some(memory_limit),
                checked,
            );
            report.measure(
                "рабочий набор сверх 0.1 (справочно)",
                &resident,
                "МБ",
                None,
                false,
            );
        }
        report.finish();
    }
}

/// Папка хранилища для [`memory_child_process`].
const CHILD_VAULT: &str = "RHIZOMIND_PERF_CHILD_VAULT";
/// Строка ответа дочернего процесса: «мс закреплённая рабочий_набор заметок».
const CHILD_MARK: &str = "RHZ-PERF-CHILD ";

/// Дочерний процесс замера [`perf_index_build_open_size_memory`]: хранилище
/// 0.1 открыто (`Vault`, список заметок `NoteList` — ссылки и быстрый
/// переход), затем открытие с готовым индексом — [`store::load`] и
/// [`store::reconcile`], как поток индекса окна. Память сверх 0.1 — разница
/// памяти процесса ([`memory`]) до и после; байты файла к замеру
/// освобождены — остаётся то, что живёт, пока окно открыто. Без
/// переменной [`CHILD_VAULT`] ничего не делает.
#[test]
#[ignore = "дочерний процесс perf_index_build_open_size_memory"]
fn memory_child_process() {
    let Some(root) = std::env::var_os(CHILD_VAULT) else {
        return;
    };
    let vault = Vault::open(std::path::Path::new(&root)).expect("хранилище");
    let list = NoteList::new(vault.notes().expect("заметки"));
    let before = memory();
    let (took, index) = timed(|| {
        let index = match store::load(&vault) {
            Load::Loaded(index) => RwLock::new(*index),
            Load::Rebuild(reason) => panic!("index.json не прочитан: {reason:?}"),
        };
        store::reconcile(&vault, &index).expect("сверка");
        index
    });
    let after = memory();
    let total = index.read().expect("индекс").entries().count();
    let (private, resident) = match (before, after) {
        (Some(b), Some(a)) => (
            a.private as f64 - b.private as f64,
            a.resident as f64 - b.resident as f64,
        ),
        _ => (f64::NAN, f64::NAN),
    };
    println!("{CHILD_MARK}{took} {private} {resident} {total}");
    drop(index);
    drop(list);
}
