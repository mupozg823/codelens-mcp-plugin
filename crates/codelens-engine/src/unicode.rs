//! #349: Unicode normalization for symbol-name matching.
//!
//! Hangul (and any combining-mark) identifiers written in NFD —
//! typically pasted from macOS filenames, where APFS preserves
//! decomposed jamo — silently miss NFC queries when names are compared
//! byte-exact. The fix is one canonical form at both boundaries: symbol
//! names are normalized to NFC once at extraction (so the index, the
//! overview payloads, and the BM25F corpus all carry NFC), and query
//! strings are normalized the same way before hitting the store.
//!
//! Signatures and bodies stay byte-faithful to the source file — only
//! identifier-matching fields normalize. Pre-existing index rows keep
//! their on-disk form until the next `refresh_symbol_index`.

use std::borrow::Cow;
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

/// NFC-normalize an identifier. ASCII (the overwhelming majority of
/// symbol names) and already-NFC strings take the zero-alloc path.
pub fn nfc_identifier(name: &str) -> Cow<'_, str> {
    if name.is_ascii() || is_nfc_quick(name.chars()) == IsNormalized::Yes {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(name.nfc().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::nfc_identifier;
    use std::borrow::Cow;

    #[test]
    fn ascii_borrows() {
        assert!(matches!(nfc_identifier("dispatch_tool"), Cow::Borrowed(_)));
    }

    #[test]
    fn nfc_hangul_borrows() {
        // Precomposed syllables — already NFC.
        assert!(matches!(nfc_identifier("후원금_정산"), Cow::Borrowed(_)));
    }

    #[test]
    fn nfd_hangul_composes_to_nfc() {
        // "후원자" decomposed into jamo (NFD) — 9 codepoints.
        let nfd = "\u{1112}\u{116e}\u{110b}\u{116f}\u{11ab}\u{110c}\u{1161}";
        let out = nfc_identifier(nfd);
        assert_eq!(out.as_ref(), "후원자");
        assert_eq!(out.chars().count(), 3);
    }
}
