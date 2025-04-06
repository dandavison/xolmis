use std::path::{Path, PathBuf};
use std::env;
use regex::Regex;

// Make the main transform function public
pub fn transform(line: &str, cwd: &Path, re: &Regex) -> String {
    // Process the whole input string - may need refinement for multi-buffer matches
    re.replace_all(line, |caps: &regex::Captures| {
        let matched_text = &caps[0];
        let rel_path = &caps[1];
        if let Ok(line_num) = caps[2].parse::<u32>() {
            let link = format_vscode_hyperlink(cwd, rel_path, line_num);
            format_osc8_hyperlink(&link, matched_text)
        } else {
            matched_text.to_string() // Not a valid number, return original
        }
    }).to_string()
}

// Keep these helper functions private to this module
fn format_vscode_hyperlink(cwd: &Path, rel_path: &str, line: u32) -> String {
    let path = cwd.join(rel_path);
    // Ensure the path is absolute before creating the URI
    let absolute_path = if path.is_absolute() {
        path
    } else {
        // This might be redundant if cwd is always absolute, but safer
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };
    format!("cursor://file/{}:{}", absolute_path.display(), line)
}

fn format_osc8_hyperlink(url: &str, text: &str) -> String {
    format!(
        "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
        url,
        text
    )
} 