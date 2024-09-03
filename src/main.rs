use regex::Regex;
use std::env::current_dir;
use std::io::{self, BufRead, Write};
use std::path::Path;

fn main() {
    let mut stdout = io::stdout().lock();
    let cwd = current_dir().unwrap();
    let cwd = cwd.as_os_str().to_str().unwrap();

    let re = Regex::new(r"^([a-zA-Z-_./]+):(\d+)").unwrap();
    for line in io::stdin().lock().lines() {
        if let Ok(line) = line {
            writeln!(stdout, "{}", transform(&line, cwd, &re)).unwrap();
        }
    }
}

fn transform(line: &str, cwd: &str, re: &Regex) -> String {
    if let Some(caps) = re.captures(line) {
        let n = caps[0].len();
        let rel_path = &caps[1];
        let line_num: u32 = caps[2].parse().unwrap();
        let link = format_vscode_hyperlink(Path::new(cwd), rel_path, line_num);
        return format!("{}{}", format_osc8_hyperlink(&link, &line[..n]), &line[n..]);
    } else {
        return line.to_string();
    }
}

fn format_vscode_hyperlink(cwd: &Path, rel_path: &str, line: u32) -> String {
    let path = cwd.join(rel_path);
    format!("cursor://file/{}:{}", path.display(), line)
}

fn format_osc8_hyperlink(url: &str, text: &str) -> String {
    format!(
        "{osc}8;;{url}{st}{text}{osc}8;;{st}",
        url = url,
        text = text,
        osc = "\x1b]",
        st = "\x1b\\"
    )
}
