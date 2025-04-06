use std::path::{Path, PathBuf};
use regex::Regex;
use lazy_static::lazy_static;

use crate::ansi::iterator::{AnsiElementIterator, Element};

lazy_static! {
    // Python: File "/path/to/file.py", line 123
    static ref PYTHON_TRACE_RE: Regex = Regex::new(r#"File "([^"]+)", line (\d+)"#).unwrap();
    // IPDB: > /path/to/file.py(123) or -> /path/to/file.py(123)
    static ref IPDB_TRACE_RE: Regex = Regex::new(r"^[->]\s*(\S+)\((\d+)\)").unwrap();
    // Compiler/General: path/to/file.py:123 or /abs/path:45 etc.
    static ref FILE_PATH_LINE_RE: Regex = Regex::new(r"([a-zA-Z0-9-_./]+):(\d+)").unwrap();
}

#[derive(Debug)]
struct MatchInfo<'a> {
    start: usize,
    end: usize,
    text: &'a str,
    path: &'a str,
    line: u32,
    #[allow(dead_code)] // Could be used for logging/debugging
    rule_name: &'static str,
}

pub fn transform(chunk: &str, cwd: &Path) -> String {
    let mut output = String::with_capacity(chunk.len()); // Start with same capacity, may grow

    for element in AnsiElementIterator::new(chunk) {
        match element {
            Element::Text(start, end) => {
                let text_segment = &chunk[start..end];
                let mut matches = Vec::new();

                // Collect matches from all rules within this segment
                collect_matches(&PYTHON_TRACE_RE, text_segment, "Python", 1, 2, &mut matches);
                collect_matches(&IPDB_TRACE_RE, text_segment, "IPDB", 1, 2, &mut matches);
                collect_matches(&FILE_PATH_LINE_RE, text_segment, "FilePathLine", 1, 2, &mut matches);

                // Sort matches by start index to process them in order
                matches.sort_by_key(|m| m.start);

                let mut last_match_end = 0;

                // Process non-overlapping matches
                for m in matches {
                    // Ensure this match doesn't overlap with the previous one we processed
                    if m.start >= last_match_end {
                         // Resolve path relative to cwd
                        let full_path = resolve_path(cwd, m.path);

                        // Heuristic check: Only create link if path exists or looks plausible
                        // (contains '/', starts with '.', or is absolute).
                        // Avoids linking things like "http:80" or "UUID:123".
                        let should_link = full_path.exists() ||
                                          m.path.contains('/') ||
                                          m.path.starts_with('.') ||
                                          Path::new(m.path).is_absolute();

                        if should_link {
                            // Append text before the match
                            output.push_str(&text_segment[last_match_end..m.start]);

                            // Format and append hyperlink
                            let link_url = format_cursor_hyperlink(&full_path, m.line);
                            let hyperlinked_text = format_osc8_hyperlink(&link_url, m.text);
                            output.push_str(&hyperlinked_text);

                            last_match_end = m.end;
                        } else {
                           // If we decided not to link, skip this match and let the text
                           // be appended normally later.
                        }
                    }
                }

                // Append remaining text after the last processed match
                output.push_str(&text_segment[last_match_end..]);

            }
            // For non-text elements (ANSI codes), append them directly
            Element::Sgr(_, s, e) |
            Element::Csi(s, e) |
            Element::Esc(s, e) |
            Element::Osc(s, e) => {
                output.push_str(&chunk[s..e]);
            }
        }
    }

    output
}

// Helper to collect matches for a given regex
fn collect_matches<'a>(
    regex: &Regex,
    text_segment: &'a str,
    rule_name: &'static str,
    path_group_index: usize,
    line_group_index: usize,
    matches: &mut Vec<MatchInfo<'a>>,
) {
    for caps in regex.captures_iter(text_segment) {
        if let (Some(match_obj), Some(path_match), Some(line_num_match)) =
            (caps.get(0), caps.get(path_group_index), caps.get(line_group_index))
        {
            if let Ok(line_num) = line_num_match.as_str().parse::<u32>() {
                // Basic check: don't add if path_match is empty
                if !path_match.as_str().is_empty() {
                    matches.push(MatchInfo {
                        start: match_obj.start(),
                        end: match_obj.end(),
                        text: match_obj.as_str(),
                        path: path_match.as_str(),
                        line: line_num,
                        rule_name,
                    });
                }
            }
        }
    }
}

// Helper to resolve path relative to cwd or handle absolute paths
fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        // TODO: Consider handling '~' expansion if needed?
        cwd.join(path)
    }
}


// Creates a hyperlink target URL (using custom cursor:// scheme for potential editor integration)
fn format_cursor_hyperlink(absolute_path: &Path, line: u32) -> String {
    // Attempt to get a canonical path, fall back to the resolved absolute path
    let canonical_path = absolute_path.canonicalize().unwrap_or_else(|_| absolute_path.to_path_buf());
    // Use to_string_lossy to handle potential non-UTF8 paths gracefully
    format!("cursor://file/{}:{}", canonical_path.to_string_lossy(), line)
}

// Formats the text with OSC 8 terminal hyperlinks
fn format_osc8_hyperlink(url: &str, text: &str) -> String {
    format!(
        "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
        url,
        text
    )
}
