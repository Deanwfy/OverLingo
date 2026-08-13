pub fn enabled() -> bool {
    std::env::var("OVERLINGO_DIAGNOSTICS")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
}

/// Seconds since the first log line, so the gaps between stages can be read directly.
fn elapsed() -> f64 {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

pub fn log(scope: &str, message: impl AsRef<str>) {
    if enabled() {
        eprintln!("[{:8.3} {scope}] {}", elapsed(), message.as_ref());
    }
}

pub fn field(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect()
}
