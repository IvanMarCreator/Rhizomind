//! Местное время для имени копии при конфликте: «ГГГГ-ММ-ДД ЧЧ-ММ» (С3.6).
//! Часовой пояс — системный, средствами ОС (без отдельного крейта времени).

use std::time::SystemTime;

/// Дата и время до минуты по местному времени; ОС не ответила — по UTC.
pub fn minute_stamp(time: SystemTime) -> String {
    let (year, month, day, hour, minute) = local(time).unwrap_or_else(|| utc(time));
    format!("{year:04}-{month:02}-{day:02} {hour:02}-{minute:02}")
}

type Parts = (i64, u32, u32, u32, u32);

fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// Разбор времени UTC без часового пояса (алгоритм дней гражданского
/// календаря, H. Hinnant).
fn utc(time: SystemTime) -> Parts {
    let secs = unix_seconds(time);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rest / 3600) as u32,
        (rest % 3600 / 60) as u32,
    )
}

#[cfg(unix)]
fn local(time: SystemTime) -> Option<Parts> {
    let secs = libc::time_t::try_from(unix_seconds(time)).ok()?;
    // SAFETY: `tm` — обычная структура C, заполняется `localtime_r`;
    // указатели живут до конца вызова.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::localtime_r(&secs, &mut tm) };
    if result.is_null() {
        return None;
    }
    Some((
        i64::from(tm.tm_year) + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    ))
}

#[cfg(windows)]
fn local(time: SystemTime) -> Option<Parts> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    // FILETIME — сотни наносекунд с 1601-01-01.
    let ticks = (unix_seconds(time) + 11_644_473_600).checked_mul(10_000_000)?;
    let ticks = u64::try_from(ticks).ok()?;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    // SAFETY: структуры заполняются функциями Win32; указатели живут до конца
    // вызова; null — текущий часовой пояс.
    let mut utc_time: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local_time: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        if FileTimeToSystemTime(&file_time, &mut utc_time) == 0
            || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc_time, &mut local_time) == 0
        {
            return None;
        }
    }
    Some((
        i64::from(local_time.wYear),
        u32::from(local_time.wMonth),
        u32::from(local_time.wDay),
        u32::from(local_time.wHour),
        u32::from(local_time.wMinute),
    ))
}

#[cfg(not(any(unix, windows)))]
fn local(_time: SystemTime) -> Option<Parts> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_parts_of_known_dates() {
        let at = |secs: u64| utc(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(at(0), (1970, 1, 1, 0, 0));
        // 2026-10-25 09:40:00 UTC
        assert_eq!(at(1_792_921_200), (2026, 10, 25, 9, 40));
        // 2024-02-29 23:59 UTC — високосный год
        assert_eq!(at(1_709_251_140), (2024, 2, 29, 23, 59));
    }

    #[test]
    fn stamp_has_the_documented_shape() {
        let stamp = minute_stamp(SystemTime::now());
        let shape: String = stamp
            .chars()
            .map(|c| if c.is_ascii_digit() { 'D' } else { c })
            .collect();
        assert_eq!(shape, "DDDD-DD-DD DD-DD");
    }
}
