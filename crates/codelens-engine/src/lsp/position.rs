pub(super) fn extract_text_for_range(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || end_line == 0 || line > lines.len() || end_line > lines.len() {
        return String::new();
    }
    if line == end_line {
        let text = lines[line - 1];
        let start = column.saturating_sub(1).min(text.len());
        let end = end_column.saturating_sub(1).min(text.len());
        return text.get(start..end).unwrap_or_default().to_owned();
    }
    let mut result = String::new();
    for index in line..=end_line {
        let text = lines[index - 1];
        let slice = if index == line {
            text.get(column.saturating_sub(1).min(text.len())..)
                .unwrap_or_default()
        } else if index == end_line {
            text.get(..end_column.saturating_sub(1).min(text.len()))
                .unwrap_or_default()
        } else {
            text
        };
        result.push_str(slice);
        if index != end_line {
            result.push('\n');
        }
    }
    result
}

pub(super) fn byte_column_for_utf16_position(
    source: &str,
    line: usize,
    character_utf16: usize,
) -> usize {
    let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
        return 1;
    };

    let mut consumed_utf16 = 0usize;
    for (byte_index, ch) in text.char_indices() {
        if consumed_utf16 >= character_utf16 {
            return byte_index + 1;
        }
        let next_utf16 = consumed_utf16 + ch.len_utf16();
        if next_utf16 > character_utf16 {
            return byte_index + 1;
        }
        consumed_utf16 = next_utf16;
    }
    text.len() + 1
}

pub(super) fn utf16_character_for_byte_column(source: &str, line: usize, column: usize) -> usize {
    let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
        return 0;
    };

    let target_byte = column.saturating_sub(1).min(text.len());
    let mut consumed_utf16 = 0usize;
    for (byte_index, ch) in text.char_indices() {
        if byte_index >= target_byte {
            return consumed_utf16;
        }
        let next_byte = byte_index + ch.len_utf8();
        if next_byte > target_byte {
            return consumed_utf16;
        }
        consumed_utf16 += ch.len_utf16();
    }
    consumed_utf16
}
