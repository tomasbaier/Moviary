//! Language of the texts the backend sends to the frontend (error messages, language names).
//! The language is chosen in the app (Czech or English) and stored in the settings.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_language(code: &str) {
    ENGLISH.store(code == "en", Ordering::Relaxed);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// `tr!("Soubor {name} chybí.", "File {name} is missing.")` – the text formatted in the
/// app's language (both accept the same `format!` arguments).
#[macro_export]
macro_rules! tr {
    ($cs:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::english() {
            format!($en $(, $arg)*)
        } else {
            format!($cs $(, $arg)*)
        }
    };
}
