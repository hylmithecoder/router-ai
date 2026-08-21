//! Verbose logger. Messages are printed to stdout with ANSI colors only — no
//! file is written, so nothing accumulates on disk. Redirect stdout to a file
//! yourself (`cargo run > out.log`) if you ever need a persisted trace.

use std::time::{SystemTime, UNIX_EPOCH};

/// `HH:MM:SS.mmm` wall-clock stamp (good enough for local dev logs).
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let secs = total_secs % 60;
    let millis = now.subsec_millis();
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, secs, millis)
}

#[doc(hidden)]
pub fn __debug_impl(msg: &str, file: &'static str, line: u32, func: &str) {
    println!(
        "\x1b[36m[{}] [DEBUG]\x1b[0m \x1b[90m{}:{} ({})\x1b[0m | {}",
        timestamp(),
        file,
        line,
        func,
        msg
    );
}

#[doc(hidden)]
pub fn __log_err_impl(msg: &str, file: &'static str, line: u32, func: &str) {
    println!(
        "\x1b[31m[{}] [ERROR]\x1b[0m \x1b[90m{}:{} ({})\x1b[0m | {}",
        timestamp(),
        file,
        line,
        func,
        msg
    );
}

#[doc(hidden)]
pub fn __log_info_impl(msg: &str, file: &'static str, line: u32, func: &str) {
    println!(
        "\x1b[32m[{}] [INFO]\x1b[0m | {}:{}({})	{}",
        timestamp(),
        file,
        line,
        func,
        msg
    );
}

#[doc(hidden)]
pub fn __log_warn_impl(msg: &str, file: &'static str, line: u32, func: &str) {
    println!(
        "\x1b[33m[{}] [WARN]\x1b[0m | {}:{}({})\t{}",
        timestamp(),
        file,
        line,
        func,
        msg
    );
}

#[macro_export]
macro_rules! function_name {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: &T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(&f);
        let name = name.strip_suffix("::f").unwrap_or(name);
        name.rsplit("::").next().unwrap_or(name)
    }};
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::utils::debugger::__debug_impl(
            &format!($($arg)*),
            file!(),
            line!(),
            $crate::function_name!(),
        )
    };
}

#[macro_export]
macro_rules! log_err {
    ($($arg:tt)*) => {
        $crate::utils::debugger::__log_err_impl(
            &format!($($arg)*),
            file!(),
            line!(),
            $crate::function_name!(),
        )
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::utils::debugger::__log_info_impl(
            &format!($($arg)*),
            file!(),
            line!(),
            $crate::function_name!(),
        )
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::utils::debugger::__log_warn_impl(
            &format!($($arg)*),
            file!(),
            line!(),
            $crate::function_name!(),
        )
    };
}
