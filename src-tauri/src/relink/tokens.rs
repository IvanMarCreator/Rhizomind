//! Знаки планов переписывания для интерфейса (задача 0.2-4.6; С2-5.5,
//! С2-6 «Граница»).
//!
//! Знак — непрозрачная строка из
//! [`RELINK_TOKEN_CHARS`](crate::commands::limits::RELINK_TOKEN_CHARS)
//! шестнадцатеричных цифр, 16 байт из криптостойкого генератора ОС
//! (`getrandom`: `BCryptGenRandom` в Windows, `getrandom(2)` в Linux).
//! Угадать чужой знак нельзя, а сам знак ничего не значит: за ним в памяти
//! ядра лежит то, что ядро посчитало само (переименование предпросмотра,
//! план повтора). Интерфейс может только вернуть знак — подтвердить или
//! отклонить.
//!
//! Знак:
//! - одноразовый — [`Tokens::take`] убирает его при любом предъявлении, в
//!   том числе неудачном (другой план); вернуть его может только ядро —
//!   [`Tokens::restore`], если действие по нему не началось (запись ввода не
//!   удалась), с прежним временем выдачи;
//! - привязан к окну — знак, предъявленный чужим окном, ищется в хранилище
//!   знаков этого (чужого) окна и не принимается: знаки другого окна там не
//!   лежат;
//! - живёт не дольше
//!   [`RELINK_TOKEN_TTL_SECS`](crate::commands::limits::RELINK_TOKEN_TTL_SECS);
//! - хранится только в памяти и пропадает с окном; на диск не пишется.
//!
//! Знаков на окно — не больше [`MAX_TOKENS`] (64): при пределе вытесняются
//! самые старые знаки (окно, которое без конца спрашивает предпросмотр, не
//! растит память).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::commands::limits::{RELINK_TOKEN_CHARS, RELINK_TOKEN_TTL_SECS};
use crate::commands::{CoreError, CoreResult, ErrorCode, RelinkToken};

/// Сколько знаков одного окна хранится одновременно.
pub const MAX_TOKENS: usize = 64;

/// Срок жизни знака.
pub const TTL: Duration = Duration::from_secs(RELINK_TOKEN_TTL_SECS);

/// Новый знак из генератора ОС. Генератор недоступен — `io_failed`
/// (предсказуемого запасного знака нет).
pub fn fresh() -> CoreResult<RelinkToken> {
    let mut bytes = [0_u8; RELINK_TOKEN_CHARS / 2];
    getrandom::fill(&mut bytes).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
    let mut text = String::with_capacity(RELINK_TOKEN_CHARS);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    Ok(RelinkToken(text))
}

/// Выданный знак: окно, время выдачи и то, что за ним стоит.
#[derive(Debug)]
struct Issued<T> {
    window: String,
    issued: Instant,
    value: T,
}

/// Знаки окна и то, что за ними стоит.
#[derive(Debug)]
pub struct Tokens<T> {
    entries: Mutex<HashMap<String, Issued<T>>>,
    /// Сдвиг часов для тестов просрочки (прибавляется к `Instant::now`).
    #[cfg(test)]
    skew: Mutex<Duration>,
}

impl<T> Default for Tokens<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            #[cfg(test)]
            skew: Mutex::default(),
        }
    }
}

impl<T> Tokens<T> {
    fn now(&self) -> Instant {
        #[cfg(test)]
        {
            Instant::now() + *self.skew.lock().unwrap_or_else(PoisonError::into_inner)
        }
        #[cfg(not(test))]
        {
            Instant::now()
        }
    }

    /// Выдаёт знак окну `window` для `value`. Просроченные знаки
    /// убираются, лишние старые вытесняются.
    pub fn issue(&self, window: &str, value: T) -> CoreResult<RelinkToken> {
        let token = fresh()?;
        let now = self.now();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, issued| !expired(issued.issued, now));
        while entries.len() >= MAX_TOKENS {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, issued)| issued.issued)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
        entries.insert(
            token.0.clone(),
            Issued {
                window: window.to_owned(),
                issued: now,
                value,
            },
        );
        Ok(token)
    }

    /// Предъявление знака окном `window`: знак убирается в любом случае;
    /// то, что за ним стоит, — только если знак выдан этому окну и не
    /// просрочен. Иначе `None` (вызывающий отвечает `invalid_argument`).
    pub fn take(&self, window: &str, token: &RelinkToken) -> Option<T> {
        self.take_issued(window, token).map(|(value, _)| value)
    }

    /// [`Tokens::take`] с временем выдачи (для [`Tokens::restore`]).
    pub fn take_issued(&self, window: &str, token: &RelinkToken) -> Option<(T, Instant)> {
        let now = self.now();
        let issued = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&token.0)?;
        (issued.window == window && !expired(issued.issued, now))
            .then_some((issued.value, issued.issued))
    }

    /// Возвращает взятый знак с прежним окном и временем выдачи — срок жизни
    /// не продлевается. Только для знака, по которому действие не началось.
    pub fn restore(&self, window: &str, token: &RelinkToken, value: T, issued: Instant) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                token.0.clone(),
                Issued {
                    window: window.to_owned(),
                    issued,
                    value,
                },
            );
    }

    /// Сколько знаков хранится (тесты).
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Часы знаков уходят вперёд на `by` (тесты просрочки).
    #[cfg(test)]
    pub fn advance(&self, by: Duration) {
        *self.skew.lock().unwrap_or_else(PoisonError::into_inner) += by;
    }
}

/// Знак, выданный в `issued`, просрочен к `now` (ровно 600 с — уже да).
fn expired(issued: Instant, now: Instant) -> bool {
    now.saturating_duration_since(issued) >= TTL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_lowercase_hex_and_random() {
        let a = fresh().expect("знак");
        let b = fresh().expect("знак");
        assert!(a.is_well_formed(), "{a:?}");
        assert_eq!(a.0.len(), 32);
        assert_ne!(a, b, "два знака подряд различаются");
    }

    #[test]
    fn token_single_use() {
        let tokens = Tokens::default();
        let token = tokens.issue("w", 7).expect("знак");
        assert_eq!(tokens.take("w", &token), Some(7));
        assert_eq!(tokens.take("w", &token), None, "повтор");
    }

    #[test]
    fn token_other_window_rejected_and_burned() {
        let tokens = Tokens::default();
        let token = tokens.issue("a", 1).expect("знак");
        assert_eq!(tokens.take("b", &token), None, "чужое окно");
        assert_eq!(tokens.take("a", &token), None, "знак сгорел");
    }

    #[test]
    fn token_expired_after_600s() {
        let tokens = Tokens::default();
        let young = tokens.issue("w", 1).expect("знак");
        tokens.advance(TTL - Duration::from_secs(1));
        assert_eq!(tokens.take("w", &young), Some(1), "599 с — ещё живой");
        let old = tokens.issue("w", 2).expect("знак");
        tokens.advance(TTL);
        assert_eq!(tokens.take("w", &old), None, "600 с — просрочен");
    }

    #[test]
    fn restored_token_keeps_issue_time() {
        let tokens = Tokens::default();
        let token = tokens.issue("w", 1).expect("знак");
        tokens.advance(TTL - Duration::from_secs(10));
        let (value, issued) = tokens.take_issued("w", &token).expect("знак");
        tokens.restore("w", &token, value, issued);
        tokens.advance(Duration::from_secs(10));
        assert_eq!(tokens.take("w", &token), None, "срок не продлён");
    }

    #[test]
    fn unknown_token_rejected() {
        let tokens: Tokens<u8> = Tokens::default();
        assert_eq!(tokens.take("w", &RelinkToken("0".repeat(32))), None);
    }

    #[test]
    fn tokens_bounded_oldest_evicted() {
        let tokens = Tokens::default();
        let first = tokens.issue("w", 0).expect("знак");
        for i in 1..=MAX_TOKENS {
            tokens.issue("w", i).expect("знак");
        }
        assert_eq!(tokens.count(), MAX_TOKENS);
        assert_eq!(tokens.take("w", &first), None, "самый старый вытеснен");
    }

    #[test]
    fn expired_tokens_dropped_on_issue() {
        let tokens = Tokens::default();
        tokens.issue("w", 0).expect("знак");
        tokens.advance(TTL);
        tokens.issue("w", 1).expect("знак");
        assert_eq!(tokens.count(), 1);
    }
}
