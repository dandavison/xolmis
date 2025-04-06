use std::path::{Path, PathBuf};
use std::env;
use regex::Regex;

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

// --- Transformation Logic ---

// Update function signature to accept only chunk and cwd
pub fn transform(chunk: &str, cwd: &Path) -> String {
    let rules = get_rules();
    let mut current_str = chunk.to_string();

    for rule in rules {
        current_str = rule.regex.replace_all(&current_str, |caps: &regex::Captures| {
            let matched_text = &caps[0];
            if rule.name == "FilePathLine" { 
                let rel_path = &caps[1];
                 if let Ok(line_num) = caps[2].parse::<u32>() {
                     let full_path = cwd.join(rel_path);
                     if full_path.exists() || rel_path.contains('/') || rel_path.starts_with('.') {
                        let link = format_vscode_hyperlink(cwd, rel_path, line_num);
                        format_osc8_hyperlink(&link, matched_text)
                     } else {
                         matched_text.to_string() 
                     }
                 } else {
                     matched_text.to_string() 
                 }
            } else {
                 matched_text.to_string() 
            }
        }).to_string();
    }
    current_str
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

fn format_osc8_hyperlink(url: &str, text: &str) -> String {
    format!(
        "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
        url,
        text
    )
} 