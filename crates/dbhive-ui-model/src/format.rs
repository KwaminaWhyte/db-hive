//! Number and text formatting shared by every shell.
//!
//! Lifted from the egui `widgets::ui_kit`.

/// Group digits so a count is readable at a glance.
///
/// `12345678` is a number you have to count; `12,345,678` is one you read.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What a tab's error holds when the user stopped its query. It is not an
/// error: views check [`is_cancelled`] and show a neutral "Cancelled" state
/// instead of the error panel, and the AI assistant does not offer to fix it.
///
/// A control character starts it so no driver message can ever equal it.
pub const CANCELLED: &str = "\u{1}dbhive:cancelled";

/// Whether an error string is the cancel sentinel rather than a failure.
pub fn is_cancelled(error: &str) -> bool {
    error == CANCELLED
}

/// The line a cancelled run shows where a result summary would be.
pub fn cancelled_summary(elapsed_ms: Option<u64>) -> String {
    match elapsed_ms {
        Some(ms) if ms < 1000 => format!("Cancelled after {ms}ms"),
        Some(ms) => format!("Cancelled after {:.2}s", ms as f64 / 1000.0),
        None => "Cancelled".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cancel_sentinel_is_recognised_and_nothing_else_is() {
        assert!(is_cancelled(CANCELLED));
        assert!(!is_cancelled("Cancelled"));
        assert!(!is_cancelled("Query execution failed: cancelled"));
        assert!(!is_cancelled(""));
    }

    #[test]
    fn cancelled_summary_says_how_long_it_ran() {
        assert_eq!(cancelled_summary(None), "Cancelled");
        assert_eq!(cancelled_summary(Some(42)), "Cancelled after 42ms");
        assert_eq!(cancelled_summary(Some(2500)), "Cancelled after 2.50s");
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
