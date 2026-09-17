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

/// Split an identifier into its words at `_` and camelCase boundaries,
/// keeping the source casing: `parseSymbols` → `parse`, `Symbols`;
/// `HTTPServer` → `HTTP`, `Server`; `build_non_code_ranges` → `build`,
/// `non`, `code`, `ranges`. An uppercase letter starts a new word when the
/// letter before it is lowercase or the letter after it is, so acronyms stay
/// whole. This is the one boundary rule shared by the BM25F tokenizer, query
/// expansion and the embedding prompt.
pub fn identifier_words(name: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start: Option<usize> = None;
    let mut prev: Option<char> = None;
    let mut chars = name.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch == '_' {
            if let Some(word_start) = start.take() {
                words.push(&name[word_start..offset]);
            }
            prev = None;
            continue;
        }
        match start {
            None => start = Some(offset),
            Some(word_start) => {
                let next_is_lowercase = chars.peek().is_some_and(|&(_, next)| next.is_lowercase());
                if ch.is_uppercase() && (prev.is_some_and(char::is_lowercase) || next_is_lowercase)
                {
                    words.push(&name[word_start..offset]);
                    start = Some(offset);
                }
            }
        }
        prev = Some(ch);
    }
    if let Some(word_start) = start {
        words.push(&name[word_start..]);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::{identifier_words, nfc_identifier};
    use std::borrow::Cow;

    #[test]
    fn identifier_words_split_camel_snake_and_acronyms() {
        assert_eq!(identifier_words("parseSymbols"), ["parse", "Symbols"]);
        assert_eq!(
            identifier_words("SparseSymbolIndex"),
            ["Sparse", "Symbol", "Index"]
        );
        assert_eq!(identifier_words("HTTPServer"), ["HTTP", "Server"]);
        assert_eq!(
            identifier_words("getHTTPResponse"),
            ["get", "HTTP", "Response"]
        );
        assert_eq!(
            identifier_words("build_non_code_ranges"),
            ["build", "non", "code", "ranges"]
        );
        assert_eq!(identifier_words("__init__"), ["init"]);
        assert_eq!(identifier_words("MAX_RESULTS"), ["MAX", "RESULTS"]);
        assert_eq!(identifier_words("utf8Decode"), ["utf8", "Decode"]);
        assert_eq!(identifier_words("후원금_정산"), ["후원금", "정산"]);
        assert!(identifier_words("").is_empty());
        assert!(identifier_words("___").is_empty());
    }

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
