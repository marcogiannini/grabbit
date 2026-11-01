// SPDX-License-Identifier: GPL-3.0-or-later
//! Lingua dell'interfaccia: italiano se il sistema è in italiano, inglese altrimenti.
//! Language: Italian if the system language is Italian, English otherwise.

use std::sync::OnceLock;

static ITALIAN: OnceLock<bool> = OnceLock::new();

fn detect() -> bool {
    // GRABBIT_LANG=it|en forza la lingua (utile per prove e assistenza)
    if let Ok(v) = std::env::var("GRABBIT_LANG") {
        return v.trim().to_ascii_lowercase().starts_with("it");
    }
    sys_locale::get_locale()
        .map(|l| l.trim().to_ascii_lowercase().starts_with("it"))
        .unwrap_or(false)
}

pub fn is_italian() -> bool {
    *ITALIAN.get_or_init(detect)
}

/// `tr!("testo italiano", "english text")`: sceglie il testo nella lingua attiva.
/// Funziona sia con `&str` sia con `format!(…)`: viene valutato solo il ramo scelto.
#[macro_export]
macro_rules! tr {
    ($it:expr, $en:expr) => {
        if $crate::i18n::is_italian() {
            $it
        } else {
            $en
        }
    };
}
