//! Генератор тестового хранилища (задача 1.6).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Параметры генерации.
#[derive(Debug, Clone)]
pub struct Config {
    pub notes: usize,
    pub seed: u64,
    pub big_mb: Option<u64>,
}

impl Config {
    pub fn new(notes: usize, seed: u64) -> Self {
        Config { notes, seed, big_mb: None }
    }
}

/// Сводка созданного хранилища.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub notes: usize,
}

pub fn generate(_out: &Path, _cfg: &Config) -> std::io::Result<Manifest> {
    Err(std::io::Error::other("не реализовано"))
}
