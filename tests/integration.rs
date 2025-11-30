use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TestSession {
    name: String,
}

impl TestSession {
    fn new() -> Self {
        let id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("xolmis_test_{}", id);

        // Build xolmis
        let status = Command::new("cargo")
            .args(["build"])
            .status()
            .expect("cargo build failed");
        assert!(status.success(), "cargo build failed");

        // Kill any existing session with this name
        let _ = tmux(&["kill-session", "-t", &name]);

        // Start tmux with xolmis
        let status = tmux(&[
            "new-session",
            "-d",
            "-s",
            &name,
            "-x",
            "80",
            "-y",
            "24",
            "./target/debug/xolmis",
        ])
        .status;
        assert!(status.success(), "failed to start tmux session");

        thread::sleep(Duration::from_millis(500));
        Self { name }
    }

    fn send_keys(&self, keys: &str) {
        tmux(&["send-keys", "-t", &self.name, keys, "Enter"]);
        thread::sleep(Duration::from_millis(300));
    }

    fn capture(&self) -> String {
        let output = tmux(&["capture-pane", "-t", &self.name, "-p"]);
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn capture_with_escapes(&self) -> String {
        let output = tmux(&["capture-pane", "-t", &self.name, "-p", "-e"]);
        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        let _ = tmux(&["kill-session", "-t", &self.name]);
    }
}

fn tmux(args: &[&str]) -> std::process::Output {
    Command::new("tmux")
        .args(args)
        .output()
        .expect("tmux command failed")
}

#[test]
fn test_hello_world() {
    let session = TestSession::new();
    session.send_keys("echo hello_from_xolmis");
    let content = session.capture();

    assert!(
        content.contains("hello_from_xolmis"),
        "expected 'hello_from_xolmis' in pane content:\n{}",
        content
    );
}

#[test]
fn test_hyperlink_insertion() {
    let session = TestSession::new();
    session.send_keys("echo src/main.rs:10");
    let content = session.capture_with_escapes();

    // OSC 8 hyperlink format: \x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\
    assert!(
        content.contains("]8;;cursor://file/"),
        "expected OSC 8 hyperlink in output:\n{}",
        content
    );
}

#[test]
fn test_nonexistent_file_no_hyperlink() {
    let session = TestSession::new();
    session.send_keys("echo /nonexistent/path/file.rs:42");
    let content = session.capture_with_escapes();

    assert!(
        !content.contains("]8;;cursor://file/"),
        "should NOT hyperlink non-existent files:\n{}",
        content
    );
}

#[test]
fn test_ansi_colors_preserved() {
    let session = TestSession::new();
    // printf with ANSI red color
    session.send_keys("printf '\\033[31mred text\\033[0m'");
    let content = session.capture_with_escapes();

    // Check that SGR codes are present (31m for red, 0m for reset)
    assert!(
        content.contains("[31m") && content.contains("[0m"),
        "ANSI color codes should be preserved:\n{}",
        content
    );
}

#[test]
fn test_python_traceback_format() {
    let session = TestSession::new();
    let cwd = std::env::current_dir().unwrap();
    // Use $'...' syntax for proper escape handling in bash/zsh
    let cmd = format!("echo $'  File \"{}/src/main.rs\", line 10'", cwd.display());
    session.send_keys(&cmd);
    thread::sleep(Duration::from_millis(100));
    let content = session.capture_with_escapes();

    assert!(
        content.contains("]8;;cursor://file/"),
        "Python traceback should generate hyperlink:\n{}",
        content
    );
}

#[test]
fn test_pager_basic() {
    let session = TestSession::new();

    // Pipe content with a path:line pattern through less
    session.send_keys("echo 'src/main.rs:10' | less");
    thread::sleep(Duration::from_millis(300));
    let content = session.capture_with_escapes();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);
    thread::sleep(Duration::from_millis(100));

    // Check that hyperlink was added even through pager
    assert!(
        content.contains("]8;;cursor://file/"),
        "pager output should contain hyperlink:\n{}",
        content
    );
}

#[test]
fn test_pager_full_screen() {
    let session = TestSession::new();

    // Generate 30 lines of content with path:line patterns
    session.send_keys("seq 1 30 | while read n; do echo \"Line $n: src/main.rs:$n\"; done | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // Verify we see content on the screen (less shows ~24 lines in 24-row terminal)
    // Count how many "Line N:" patterns appear
    let line_count = content.matches("Line ").count();
    assert!(
        line_count >= 20,
        "expected at least 20 lines visible in pager, got {}:\n{}",
        line_count,
        content
    );
}

#[test]
#[ignore = "requires SIGWINCH handling (not yet implemented)"]
fn test_terminal_resize() {
    let session = TestSession::new();

    // Get initial size
    session.send_keys("tput cols; tput lines");
    thread::sleep(Duration::from_millis(100));
    let content_before = session.capture();

    // Parse initial dimensions
    let lines: Vec<&str> = content_before.lines().collect();
    let initial_cols: Option<u32> = lines.iter().filter_map(|l| l.trim().parse().ok()).next();

    // Resize the tmux pane to a different size
    let new_cols = initial_cols.unwrap_or(80) + 20;
    let new_lines = 30;
    tmux(&[
        "resize-pane",
        "-t",
        &session.name,
        "-x",
        &new_cols.to_string(),
        "-y",
        &new_lines.to_string(),
    ]);
    thread::sleep(Duration::from_millis(300));

    // Check size after resize
    session.send_keys("echo \"AFTER_RESIZE: $(tput cols)x$(tput lines)\"");
    thread::sleep(Duration::from_millis(100));
    let content_after = session.capture();

    // After resize, PTY should report new size
    // This requires SIGWINCH handling which is not yet implemented
    let expected = format!("AFTER_RESIZE: {}x{}", new_cols, new_lines);
    assert!(
        content_after.contains(&expected),
        "terminal resize not propagated to PTY (SIGWINCH handling needed).\nExpected: {}\nGot:\n{}",
        expected,
        content_after
    );
}
