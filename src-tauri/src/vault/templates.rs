//! Создание самостоятельной Markdown-заметки из шаблона открытого хранилища.
use super::Vault;
use crate::commands::{CoreResult, VaultPath};

#[cfg(any(windows, target_os = "linux"))]
use super::copy::CopyInput;

#[cfg(any(windows, target_os = "linux"))]
impl Vault {
    /// Исходник читается через открытый дескриптор, без разбора Markdown и
    /// загрузки всего файла в память. Имя получает лишь готовая копия.
    pub fn create_from_template(
        &self,
        template: &VaultPath,
        dir: &VaultPath,
        name: &str,
    ) -> CoreResult<Option<VaultPath>> {
        use super::{check_name, child, contour, require_note_name};

        check_name(name)?;
        dir.check()?;
        template.check()?;
        if dir.components().iter().any(|part| part.starts_with('.')) || name.starts_with('.') {
            return Err(contour::invalid_path());
        }
        let folder = self
            .read_settings()?
            .template_folder
            .ok_or_else(contour::invalid_path)?;
        let parts = template.components();
        if parts.len() <= folder.components().len()
            || !parts.starts_with(folder.components())
            || parts.iter().any(|part| part.starts_with('.'))
        {
            return Err(contour::invalid_path());
        }
        require_note_name(template)?;
        let path = child(dir, name);
        require_note_name(&path)?;

        let copied = self.copy_user_file(CopyInput::File(template), dir, name)?;
        Ok(copied.filter(|path| require_note_name(path).is_ok()))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
impl Vault {
    pub fn create_from_template(
        &self,
        _template: &VaultPath,
        _dir: &VaultPath,
        _name: &str,
    ) -> CoreResult<Option<VaultPath>> {
        Err(crate::commands::CoreError::new(
            crate::commands::ErrorCode::NotImplemented,
        ))
    }
}
