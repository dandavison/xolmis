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
            "new-session", "-d", "-s", &name, "-x", "80", "-y", "24",
            "./target/debug/xolmis",
        ]).status;
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
    let cmd = format!(
        "echo $'  File \"{}/src/main.rs\", line 10'",
        cwd.display()
    );
    session.send_keys(&cmd);
    let content = session.capture_with_escapes();

    assert!(
        content.contains("]8;;cursor://file/"),
        "Python traceback should generate hyperlink:\n{}",
        content
    );
}
