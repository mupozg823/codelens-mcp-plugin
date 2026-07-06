use super::auto_hint::api_calls_enabled;
use super::hint::join_hint_lines;

pub fn is_static_method_ident(ident: &str) -> bool {
    ident.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

pub fn extract_api_calls(source: &str, start: usize, end: usize) -> Option<String> {
    if !api_calls_enabled() {
        return None;
    }
    extract_api_calls_inner(source, start, end)
}

pub fn extract_api_calls_inner(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let bytes = body.as_bytes();
    let len = bytes.len();

    let mut calls: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        if !(b == b'_' || b.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let ident_start = i;
        while i < len {
            let bb = bytes[i];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                i += 1;
            } else {
                break;
            }
        }
        let ident_end = i;

        if i + 1 >= len || bytes[i] != b':' || bytes[i + 1] != b':' {
            continue;
        }

        let type_ident = &body[ident_start..ident_end];
        if !is_static_method_ident(type_ident) {
            i += 2;
            continue;
        }

        let mut j = i + 2;
        if j >= len || !(bytes[j] == b'_' || bytes[j].is_ascii_alphabetic()) {
            i = j;
            continue;
        }
        let method_start = j;
        while j < len {
            let bb = bytes[j];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                j += 1;
            } else {
                break;
            }
        }
        let method_end = j;

        let method_ident = &body[method_start..method_end];
        let call = format!("{type_ident}::{method_ident}");
        if seen.insert(call.clone()) {
            calls.push(call);
        }
        i = j;
    }

    if calls.is_empty() {
        return None;
    }
    Some(join_hint_lines(&calls))
}
