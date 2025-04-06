use std::path::{Path, PathBuf};

// Use the updated types from the rules module
use crate::rules::{CompiledRule, get_compiled_rules};

use crate::ansi::iterator::{AnsiElementIterator, Element};

#[derive(Debug)]
struct MatchInfo<'a> {
    start: usize,
    end: usize,
    text: &'a str,
    path: &'a str,
    line: u32,
    #[allow(dead_code)] // Allow this field to be unused for now
    rule_name: &'static str,
}

pub fn transform(chunk: &str, cwd: &Path) -> String {
    let mut output = String::with_capacity(chunk.len());
    // Get the compiled rules
    let available_rules = get_compiled_rules();

    for element in AnsiElementIterator::new(chunk) {
        match element {
            Element::Text(start, end) => {
                let text_segment = &chunk[start..end];
                let mut matches = Vec::new();

                // Collect matches by iterating through the compiled rules
                for rule in available_rules {
                    collect_matches(rule, text_segment, &mut matches);
                }

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

// Updated helper to use CompiledRule struct
fn collect_matches<'a>(
    rule: &CompiledRule,
    text_segment: &'a str,
    matches: &mut Vec<MatchInfo<'a>>,
) {
    for caps in rule.regex.captures_iter(text_segment) {
        if let (Some(match_obj), Some(path_match), Some(line_num_match)) =
            (caps.get(0), caps.get(rule.path_group_index), caps.get(rule.line_group_index))
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
                        rule_name: rule.name,
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
