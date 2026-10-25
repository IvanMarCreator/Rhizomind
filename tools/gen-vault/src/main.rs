//! gen-vault — генератор тестового хранилища Rhizomind.
//!
//! cargo run -p gen-vault -- --notes N --out <папка> [--seed S] [--big-mb M] [--manifest <файл.json>]

use gen_vault::{Config, generate};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "\
Использование: gen-vault --notes N --out <папка> [--seed S] [--big-mb M] [--manifest <файл.json>]

  --notes N        число заметок (.md), включая образцы
  --out <папка>    куда создать хранилище; папки нет или она пуста
  --seed S         зерно (по умолчанию 1): одинаковое зерно — одинаковое хранилище
  --big-mb M       добавить заметку больше M МБ (проверка лимита размера)
  --manifest <ф>   записать сводку в JSON (вне папки хранилища)";

struct Args {
    cfg: Config,
    out: PathBuf,
    manifest: Option<PathBuf>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut notes, mut out, mut seed, mut big_mb, mut manifest) = (None, None, 1u64, None, None);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag}: нет значения"));
        let number = |v: String| {
            v.parse::<u64>()
                .map_err(|_| format!("{flag}: «{v}» не число"))
        };
        match flag.as_str() {
            "--notes" => notes = Some(number(value()?)? as usize),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--seed" => seed = number(value()?)?,
            "--big-mb" => big_mb = Some(number(value()?)?),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            _ => return Err(format!("неизвестный параметр {flag}")),
        }
    }
    let notes = notes.ok_or("нужен --notes")?;
    let out = out.ok_or("нужен --out")?;
    if let Some(m) = &manifest
        && absolute(m).starts_with(absolute(&out))
    {
        return Err("--manifest: файл внутри хранилища попал бы в его состав".into());
    }
    Ok(Args {
        cfg: Config {
            notes,
            seed,
            big_mb,
        },
        out,
        manifest,
    })
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("gen-vault: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let started = Instant::now();
    let manifest = match generate(&args.out, &args.cfg) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("gen-vault: {e}");
            return ExitCode::from(1);
        }
    };
    let elapsed = started.elapsed();
    if let Some(path) = &args.manifest {
        let json = serde_json::to_string_pretty(&manifest).expect("манифест сериализуется");
        if let Err(e) = std::fs::write(path, json + "\n") {
            eprintln!("gen-vault: {}: {e}", path.display());
            return ExitCode::from(1);
        }
    }
    println!(
        "gen-vault: {} заметок, {} папок, {} вложений, {} ссылок ({} неразрешённых), {} групп дубликатов — {} за {:.2} с",
        manifest.notes,
        manifest.folders,
        manifest.attachments.png + manifest.attachments.pdf,
        manifest.links.total,
        manifest.links.unresolved,
        manifest.duplicates.len(),
        args.out.display(),
        elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}
