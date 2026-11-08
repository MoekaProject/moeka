//! Font families and font files available on the host platform.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Family name that resolves to the platform's UI font at render time.
pub(crate) const SYSTEM_UI: &str = ".SystemUIFont";

pub(crate) fn intern_family(family: &str) -> &'static str {
    static FAMILIES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut families = FAMILIES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(&interned) = families.get(family) {
        return interned;
    }
    let interned = Box::leak(family.to_string().into_boxed_str());
    families.insert(interned);
    interned
}

#[cfg(target_os = "macos")]
pub(crate) const MONOSPACE: &str = "Menlo";
#[cfg(windows)]
pub(crate) const MONOSPACE: &str = "Consolas";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const MONOSPACE: &str = "DejaVu Sans Mono";

#[cfg(target_os = "macos")]
pub(crate) const CJK: &str = "PingFang SC";
#[cfg(windows)]
pub(crate) const CJK: &str = "Microsoft YaHei UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const CJK: &str = "Noto Sans CJK SC";

/// CJK face used for italics, which needs a family shipping an oblique cut.
#[cfg(target_os = "macos")]
pub(crate) const ITALIC_CJK: &str = "Maple Mono NF CN";
#[cfg(windows)]
pub(crate) const ITALIC_CJK: &str = "Microsoft YaHei UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const ITALIC_CJK: &str = "Noto Sans CJK SC";

/// Proportional face used where a concrete family name is required.
#[cfg(target_os = "macos")]
pub(crate) const PROPORTIONAL: &str = "Helvetica Neue";
#[cfg(windows)]
pub(crate) const PROPORTIONAL: &str = "Segoe UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const PROPORTIONAL: &str = "DejaVu Sans";

#[cfg(test)]
mod tests {
    use super::intern_family;

    #[test]
    fn intern_family_reuses_the_same_allocation() {
        let first = intern_family("Moeka Test Font");
        let second = intern_family(&"Moeka Test Font".to_string());
        assert!(std::ptr::eq(first, second));
    }
}
