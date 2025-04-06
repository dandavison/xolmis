use std::path::{Path, PathBuf};
use std::env;
use regex::Regex;

// Import the ANSI iterator types from the sibling module
use crate::ansi::iterator::{AnsiElementIterator, Element};

// --- Rule Definition ---
pub struct Rule {
    pub name: String,
    pub regex: Regex,
}

fn get_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "FilePathLine".to_string(),
            regex: Regex::new(r"([a-zA-Z0-9-_./]+):(\d+)").unwrap(),
        },
    ]
}

// --- Transformation Logic using ANSI Iterator ---
pub fn transform(chunk: &str, cwd: &Path) -> String {
    let rules = get_rules();
    let mut output = String::with_capacity(chunk.len() * 2); // Pre-allocate

    // Assume only FilePathLine rule for now
    let rule = rules.iter().find(|r| r.name == "FilePathLine").unwrap(); // TODO: Handle multiple rules more generally

    for element in AnsiElementIterator::new(chunk) {
        match element {
            Element::Text(start, end) => {
                let text_segment = &chunk[start..end];
                let mut last_match_end = 0;

                // Find matches *within* this text segment
                for caps in rule.regex.captures_iter(text_segment) {
                     if let (Some(match_obj), Some(rel_path_match), Some(line_num_match)) = (caps.get(0), caps.get(1), caps.get(2)) {
                        let match_start = match_obj.start();
                        let match_end = match_obj.end();
                        let matched_text = match_obj.as_str();
                        let rel_path = rel_path_match.as_str();

                        if let Ok(line_num) = line_num_match.as_str().parse::<u32>() {
                            // --- Path validity check ---
                            let full_path = cwd.join(rel_path);
                             if !(full_path.exists() || rel_path.contains('/') || rel_path.starts_with('.')) {
                                continue; // Skip if it doesn't look like a path
                            }
                            // --- End path check ---

                            // Append text segment before the match
                            output.push_str(&text_segment[last_match_end..match_start]);

                            // Append hyperlink
                            let link = format_vscode_hyperlink(cwd, rel_path, line_num);
                            let osc_start = format!("\x1b]8;;{}\x1b\\", link);
                            let osc_end = "\x1b]8;;\x1b\\";
                            output.push_str(&osc_start);
                            output.push_str(matched_text);
                            output.push_str(osc_end);

                            // Update position within the segment
                            last_match_end = match_end;
                        }
                    }
                }
                // Append any remaining text from the segment after the last match
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

// --- Helper Functions (Keep private) ---

fn format_vscode_hyperlink(cwd: &Path, rel_path: &str, line: u32) -> String {
    let path = cwd.join(rel_path);
    let absolute_path = if path.is_absolute() {
        path
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };
    format!("cursor://file/{}:{}", absolute_path.to_string_lossy(), line)
}

// This function is no longer called directly from transform, as the OSC codes
// are constructed inline. It can be removed or kept for potential future use.
// fn format_osc8_hyperlink(url: &str, text: &str) -> String { ... } 