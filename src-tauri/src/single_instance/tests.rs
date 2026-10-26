use std::cell::Cell;
use std::time::Duration;

use super::*;

/// Часы теста: «сон» только сдвигает время.
struct FakeClock {
    now: Cell<Duration>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            now: Cell::new(Duration::ZERO),
        }
    }
    fn elapsed(&self) -> Duration {
        self.now.get()
    }
    fn sleep(&self, step: Duration) {
        self.now.set(self.now.get() + step);
    }
}

#[test]
fn free_name_makes_this_process_the_primary() {
    let clock = FakeClock::new();
    let outcome = claim_with(
        || Ok(Some("замок")),
        || panic!("первому экземпляру некого будить"),
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::Primary(Some("замок")));
}

#[test]
fn busy_name_wakes_the_running_instance_and_this_process_exits() {
    let clock = FakeClock::new();
    let mut notified = 0;
    let outcome = claim_with(
        || Ok(None::<()>),
        || {
            notified += 1;
            true
        },
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::Notified);
    assert_eq!(notified, 1);
}

#[test]
fn instance_that_is_still_starting_is_retried_until_it_answers() {
    // Мьютекс уже есть, а окна для сообщений ещё нет (первый экземпляр
    // запускается): повтор, пока не ответит.
    let clock = FakeClock::new();
    let attempts = Cell::new(0);
    let outcome = claim_with(
        || Ok(None::<()>),
        || {
            attempts.set(attempts.get() + 1);
            attempts.get() == 3
        },
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::Notified);
    assert_eq!(attempts.get(), 3);
}

#[test]
fn instance_that_exits_meanwhile_hands_over_the_name() {
    // Первый экземпляр завершается: имя освобождается — этот процесс
    // становится первым, а не выходит впустую.
    let clock = FakeClock::new();
    let tries = Cell::new(0);
    let outcome = claim_with(
        || {
            tries.set(tries.get() + 1);
            Ok((tries.get() == 4).then_some(7))
        },
        || false,
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::Primary(Some(7)));
}

#[test]
fn silent_instance_is_given_up_after_the_timeout() {
    let clock = FakeClock::new();
    let outcome = claim_with(
        || Ok(None::<()>),
        || false,
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::GaveUp);
    assert!(clock.elapsed() >= Duration::from_secs(5));
    assert!(clock.elapsed() < Duration::from_secs(6));
}

#[test]
fn check_that_cannot_run_does_not_lock_the_user_out() {
    // Объект ОС не создаётся (нет прав, нет папки профиля): программа
    // запускается как первая, без защиты от второго экземпляра.
    let clock = FakeClock::new();
    let outcome = claim_with(
        || Err::<Option<()>, _>(std::io::Error::other("нет доступа")),
        || panic!("не будить"),
        Duration::from_secs(5),
        || clock.elapsed(),
        |step| clock.sleep(step),
    );
    assert_eq!(outcome, Outcome::Primary(None));
}

#[test]
fn names_are_constant_and_hold_no_paths_or_vault_names() {
    for name in [NAME, LOCK_FILE] {
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'),
            "{name}"
        );
        assert!(name.starts_with("Rhizomind") || name.starts_with("instance"));
    }
    if cfg!(feature = "webdriver") {
        assert_eq!(NAME, "Rhizomind.SingleInstance.webdriver");
        assert_eq!(LOCK_FILE, "instance-webdriver.lock");
    } else {
        assert_eq!(NAME, "Rhizomind.SingleInstance");
        assert_eq!(LOCK_FILE, "instance.lock");
    }
}

#[test]
fn last_focused_visible_window_is_brought_forward() {
    let mut recent = RecentWindows::default();
    recent.focused("vault-a");
    recent.focused("vault-b");
    recent.focused("vault-a");
    let visible = ["vault-b".to_owned(), "vault-a".to_owned()];
    assert_eq!(recent.target(&visible).as_deref(), Some("vault-a"));
}

#[test]
fn closed_window_is_forgotten_and_the_previous_one_is_used() {
    let mut recent = RecentWindows::default();
    recent.focused("vault-a");
    recent.focused("vault-b");
    recent.closed("vault-b");
    let visible = ["vault-a".to_owned()];
    assert_eq!(recent.target(&visible).as_deref(), Some("vault-a"));
}

#[test]
fn hidden_window_is_never_the_target() {
    // Окно Windows скрыто до проверки InPrivate (lib.rs): вывести вперёд
    // можно только уже показанное.
    let mut recent = RecentWindows::default();
    recent.focused("vault-a");
    recent.focused("vault-new");
    let visible = ["vault-a".to_owned()];
    assert_eq!(recent.target(&visible).as_deref(), Some("vault-a"));
}

#[test]
fn without_focus_history_any_visible_window_is_used() {
    let recent = RecentWindows::default();
    assert_eq!(
        recent.target(&["vault-x".to_owned()]).as_deref(),
        Some("vault-x")
    );
    assert_eq!(recent.target(&[]), None);
}

#[cfg(unix)]
mod unix_lock {
    use super::super::unix::{lock_dir, try_acquire_at};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rhz-single-{tag}-{:016x}",
            crate::webview_traces::random_u64()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_lock_on_the_same_file_is_refused_until_the_first_is_released() {
        let dir = temp_dir("lock");
        let path = dir.join("instance.lock");
        let first = try_acquire_at(&path).expect("замок").expect("первый");
        assert!(try_acquire_at(&path).expect("проверка").is_none());
        drop(first);
        let again = try_acquire_at(&path).expect("замок").expect("снова первый");
        drop(again);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lock_file_stays_empty_and_is_created_with_its_folder() {
        let dir = temp_dir("empty");
        let path = dir.join("Rhizomind").join("instance.lock");
        let guard = try_acquire_at(&path).expect("замок").expect("первый");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        // Второй запуск файл не меняет.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(try_acquire_at(&path).unwrap().is_none());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        drop(guard);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lock_lives_next_to_the_vault_list_in_the_local_profile() {
        if cfg!(feature = "webdriver") {
            return; // тестовая сборка: папка профиля сценарного теста
        }
        let expected = dirs::data_local_dir().map(|d| d.join(crate::registry::APP_DIR));
        assert_eq!(lock_dir(), expected);
    }
}

#[cfg(windows)]
mod windows_mutex {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::super::windows::{Listener, notify_named, pump_messages_until, try_acquire_named};

    fn unique(tag: &str) -> String {
        format!(
            "Rhizomind.Test.{tag}.{:016x}",
            crate::webview_traces::random_u64()
        )
    }

    #[test]
    fn second_mutex_with_the_same_name_is_refused_until_the_first_is_closed() {
        let name = unique("mutex");
        let first = try_acquire_named(&name).expect("мьютекс").expect("первый");
        assert!(try_acquire_named(&name).expect("проверка").is_none());
        drop(first);
        let again = try_acquire_named(&name)
            .expect("мьютекс")
            .expect("снова первый");
        drop(again);
    }

    #[test]
    fn running_instance_is_woken_through_its_message_window() {
        let class = unique("window");
        let (ready_tx, ready_rx) = mpsc::channel();
        let (woken_tx, woken_rx) = mpsc::channel();
        let listener_class = class.clone();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_listener = stop.clone();
        // Окно для сообщений живёт в потоке со своей очередью сообщений, как
        // в программе — в главном потоке цикла событий.
        let thread = std::thread::spawn(move || {
            let listener = Listener::start_named(
                &listener_class,
                Box::new(move || {
                    let _ = woken_tx.send(());
                }),
            )
            .expect("окно для сообщений");
            ready_tx.send(()).unwrap();
            pump_messages_until(Duration::from_secs(10), || {
                stop_listener.load(std::sync::atomic::Ordering::SeqCst)
            });
            drop(listener);
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("окно создано");

        assert!(notify_named(&class), "сигнал не доставлен");
        woken_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("первый экземпляр не получил сигнал");
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        thread.join().expect("поток окна");
    }

    #[test]
    fn nobody_to_wake_without_a_message_window() {
        assert!(!notify_named(&unique("absent")));
    }
}
