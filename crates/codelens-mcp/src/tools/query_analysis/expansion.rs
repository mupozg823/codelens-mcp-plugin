use super::intent::{has_builder_cue, specific_find_aliases};

pub(crate) fn expand_retrieval_query(query: &str) -> String {
    let lowered = query.to_lowercase();
    let mut terms = vec![query.trim().to_owned()];
    let mut push_unique = |term: &str| {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_owned());
        }
    };

    let words: Vec<&str> = lowered.split_whitespace().filter(|w| w.len() > 2).collect();
    if words.len() >= 2 && words.len() <= 6 {
        for window in words.windows(2) {
            push_unique(&format!("{}_{}", window[0], window[1]));
        }
        if words.len() >= 3 {
            for window in words.windows(3) {
                push_unique(&format!("{}_{}_{}", window[0], window[1], window[2]));
            }
        }
        let camel: String = words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.to_string()
                } else {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                }
            })
            .collect();
        push_unique(&camel);
        if words.len() >= 2 {
            let pascal: String = words
                .iter()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                })
                .collect();
            push_unique(&pascal);
        }
    }
    if query.contains('_') && !query.contains(' ') {
        let parts: Vec<&str> = query.split('_').filter(|p| !p.is_empty()).collect();
        let camel: String = parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    p.to_lowercase()
                } else {
                    let mut c = p.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + &c.as_str().to_lowercase(),
                    }
                }
            })
            .collect();
        push_unique(&camel);
    }
    if query.chars().any(|c| c.is_uppercase()) && !query.contains(' ') {
        let snake = query
            .chars()
            .enumerate()
            .fold(String::new(), |mut acc, (i, c)| {
                if c.is_uppercase() && i > 0 {
                    acc.push('_');
                }
                acc.push(c.to_ascii_lowercase());
                acc
            });
        push_unique(&snake);
    }

    if lowered.contains("route")
        || lowered.contains("request")
        || lowered.contains("handler")
        || lowered.contains("tool call")
    {
        for alias in [
            "dispatch_tool",
            "dispatch_tool_request",
            "dispatch",
            "handler",
        ] {
            push_unique(alias);
        }
    }
    if lowered.contains("move")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["move_symbol", "move"] {
            push_unique(alias);
        }
    }
    if lowered.contains("rename")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["rename_symbol", "rename"] {
            push_unique(alias);
        }
    }
    if lowered.contains("inline")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["inline_function", "inline"] {
            push_unique(alias);
        }
    }
    for alias in specific_find_aliases(&lowered) {
        push_unique(alias);
    }
    // word-match / grep-all / rename-occurrences helper queries
    if lowered.contains("word match")
        || lowered.contains("word_match")
        || lowered.contains("all occurrences")
        || lowered.contains("grep all")
        || (lowered.contains("find") && lowered.contains("match"))
    {
        for alias in [
            "find_all_word_matches",
            "find_word_matches_in_files",
            "word_match",
        ] {
            push_unique(alias);
        }
    }
    if lowered.contains("stdin") || lowered.contains("stdio") || lowered.contains("read input") {
        for alias in ["run_stdio", "stdio", "stdin"] {
            push_unique(alias);
        }
    }
    if lowered.contains("defined") || lowered.contains("definition") {
        for alias in ["find_symbol_range", "definition"] {
            push_unique(alias);
        }
    }
    if lowered.contains("change function parameters")
        || (lowered.contains("change") && lowered.contains("signature"))
        || (lowered.contains("function") && lowered.contains("parameters"))
    {
        for alias in ["change_signature", "signature"] {
            push_unique(alias);
        }
    }
    if has_builder_cue(&lowered) && lowered.contains("embedding") && lowered.contains("text") {
        for alias in ["build_embedding_text", "embedding_text"] {
            push_unique(alias);
        }
    }
    // Disambiguation aliases for generic-named symbols in specific domains
    if lowered.contains("index") && lowered.contains("project") && lowered.contains("embedding") {
        push_unique("index_from_project");
    }
    if lowered.contains("extract") && lowered.contains("call") {
        push_unique("extract_calls");
    }
    if lowered.contains("collect") && lowered.contains("candidate") {
        push_unique("collect_candidate_files");
    }

    terms.join(" ")
}
