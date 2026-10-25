//! Модель «дерево хранилища».

use std::io;
use std::path::Path;

use super::own_writes::OwnWrites;
use crate::commands::{EntryKind, TreeChange, VaultPath};

/// Модель дерева хранилища.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TreeModel {}

impl TreeModel {
    pub(crate) fn scan(_root: &Path) -> io::Result<Self> {
        Ok(Self {})
    }

    pub(crate) fn entries(&self) -> Vec<(VaultPath, EntryKind)> {
        Vec::new()
    }

    pub(crate) fn reconcile(
        &mut self,
        _paths: impl IntoIterator<Item = VaultPath>,
        _own: &OwnWrites,
    ) -> Vec<TreeChange> {
        Vec::new()
    }

    pub(crate) fn rescan(&mut self, _own: &OwnWrites) -> Vec<TreeChange> {
        Vec::new()
    }
}
