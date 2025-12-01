use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;
use std::thread;
use std::time::Duration;

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(0);
static BUILD_ONCE: Once = Once::new();

fn ensure_built() {
    BUILD_ONCE.call_once(|| {
        let status = Command::new("cargo")
            .args(["build"])
            .status()
            .expect("cargo build failed");
        assert!(status.success(), "cargo build failed");
    });
}

struct TestSession {
    name: String,
}

impl TestSession {
    fn new() -> Self {
        ensure_built();

        let id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("xolmis_test_{}", id);

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
        thread::sleep(Duration::from_millis(500));
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

fn tmux_capture(target: &str) -> String {
    let output = tmux(&["capture-pane", "-t", target, "-p"]);
    String::from_utf8_lossy(&output.stdout).to_string()
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
    thread::sleep(Duration::from_millis(200));
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
fn test_seq_in_less() {
    let session = TestSession::new();

    // Pipe seq 1-50 through less
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(800));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // In a 24-line terminal, less should show lines 1-23ish
    // Verify we see sequential numbers
    let has_1 = content.contains("\n1\n") || content.starts_with("1\n");
    let has_10 = content.contains("\n10\n");
    let has_20 = content.contains("\n20\n");

    assert!(
        has_1 && has_10 && has_20,
        "less should display seq output (1, 10, 20 expected):\n{}",
        content
    );
}

#[test]
fn test_after_load_echo() {
    let session = TestSession::new();

    // Generate significant load: 1000 lines of output with path patterns
    session.send_keys("for i in $(seq 1 1000); do echo \"load line $i: src/main.rs:$i\"; done");
    thread::sleep(Duration::from_millis(2000)); // Wait for all output

    // Now do a simple echo
    session.send_keys("echo AFTER_LOAD_MARKER_12345");
    let content = session.capture();

    assert!(
        content.contains("AFTER_LOAD_MARKER_12345"),
        "echo after load should work:\n{}",
        content
    );
}

#[test]
fn test_after_load_less() {
    let session = TestSession::new();

    // Generate significant load: 1000 lines of output with path patterns
    session.send_keys("for i in $(seq 1 1000); do echo \"load line $i: src/main.rs:$i\"; done");
    thread::sleep(Duration::from_millis(2000)); // Wait for all output

    // Now pipe seq through less
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // Verify less shows content properly after load
    let has_1 = content.contains("\n1\n") || content.starts_with("1\n");
    let has_10 = content.contains("\n10\n");
    let has_20 = content.contains("\n20\n");

    assert!(
        has_1 && has_10 && has_20,
        "less after load should display seq output (1, 10, 20 expected):\n{}",
        content
    );
}

#[test]
fn test_heavy_load_then_less() {
    let session = TestSession::new();

    // Generate heavy load: 5000 lines with path patterns
    session.send_keys("for i in $(seq 1 5000); do echo \"heavy load $i: src/main.rs:$i\"; done");
    thread::sleep(Duration::from_millis(5000)); // Wait for output

    // Pipe seq through less
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // Count how many numbers we see (should see 1-20+ in a 24-line terminal)
    let visible_numbers: Vec<u32> = (1..=30)
        .filter(|&n| {
            let pattern = format!("\n{}\n", n);
            content.contains(&pattern) || content.starts_with(&format!("{}\n", n))
        })
        .collect();

    assert!(
        visible_numbers.len() >= 15,
        "after heavy load, less should show at least 15 numbers, got {:?}:\n{}",
        visible_numbers,
        content
    );
}

#[test]
fn test_sustained_load_then_less() {
    let session = TestSession::new();

    // Multiple rounds of load with small delays (simulates sustained usage)
    for round in 1..=5 {
        let cmd = format!(
            "for i in $(seq 1 500); do echo \"round {} line $i: src/main.rs:$i\"; done",
            round
        );
        session.send_keys(&cmd);
        thread::sleep(Duration::from_millis(1000));
    }

    // Now test less
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // Verify less shows content properly
    let visible_numbers: Vec<u32> = (1..=30)
        .filter(|&n| {
            let pattern = format!("\n{}\n", n);
            content.contains(&pattern) || content.starts_with(&format!("{}\n", n))
        })
        .collect();

    assert!(
        visible_numbers.len() >= 15,
        "after sustained load, less should show at least 15 numbers, got {:?}:\n{}",
        visible_numbers,
        content
    );
}

#[test]
fn test_delta_ansi_load() {
    let session = TestSession::new();

    // Run git log through delta - generates lots of ANSI escape sequences
    // Use delta from PATH, or fall back to homebrew location
    session.send_keys(
        "git log -p -20 | delta --no-gitconfig --paging=never 2>/dev/null || \
         git log -p -20 | /opt/homebrew/bin/delta --no-gitconfig --paging=never",
    );
    thread::sleep(Duration::from_millis(3000)); // Wait for delta output

    // Now test that less still works after processing all that ANSI
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    // Verify less shows content properly after delta load
    let visible_numbers: Vec<u32> = (1..=30)
        .filter(|&n| {
            let pattern = format!("\n{}\n", n);
            content.contains(&pattern) || content.starts_with(&format!("{}\n", n))
        })
        .collect();

    assert!(
        visible_numbers.len() >= 15,
        "after delta load, less should show at least 15 numbers, got {:?}:\n{}",
        visible_numbers,
        content
    );
}

#[test]
fn test_delta_heavy_then_less() {
    let session = TestSession::new();

    // Run git log through delta multiple times
    for _ in 1..=3 {
        session.send_keys("git log | delta --no-gitconfig --paging=never");
        thread::sleep(Duration::from_millis(2000));
    }

    // Test less after heavy delta usage
    session.send_keys("seq 1 50 | less");
    thread::sleep(Duration::from_millis(500));
    let content = session.capture();

    // Quit less
    tmux(&["send-keys", "-t", &session.name, "q"]);

    let visible_numbers: Vec<u32> = (1..=30)
        .filter(|&n| {
            let pattern = format!("\n{}\n", n);
            content.contains(&pattern) || content.starts_with(&format!("{}\n", n))
        })
        .collect();

    assert!(
        visible_numbers.len() >= 15,
        "after heavy delta, less should show at least 15 numbers, got {:?}:\n{}",
        visible_numbers,
        content
    );
}

/// Test terminal resize via tmux resize-window
#[test]
fn test_terminal_resize_window() {
    let session = TestSession::new();

    // Get initial size
    session.send_keys("tput cols; tput lines");
    thread::sleep(Duration::from_millis(100));
    let content_before = session.capture();

    // Parse initial dimensions
    let lines: Vec<&str> = content_before.lines().collect();
    let initial_cols: Option<u32> = lines.iter().filter_map(|l| l.trim().parse().ok()).next();

    // Resize the tmux window
    let new_cols = initial_cols.unwrap_or(80) + 20;
    let new_lines = 30;
    tmux(&[
        "resize-window",
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

    let expected = format!("AFTER_RESIZE: {}x{}", new_cols, new_lines);
    assert!(
        content_after.contains(&expected),
        "resize-window not propagated to PTY.\nExpected: {}\nGot:\n{}",
        expected,
        content_after
    );
}

/// Test terminal resize via tmux resize-pane (requires split panes)
#[test]
fn test_terminal_resize_pane() {
    let session = TestSession::new();

    // Make the window large first
    tmux(&[
        "resize-window",
        "-t",
        &session.name,
        "-x",
        "200",
        "-y",
        "50",
    ]);
    thread::sleep(Duration::from_millis(200));

    // Split the window horizontally - creates a second pane, allowing resize-pane to work
    // Note: split-window makes the new pane active, so we must explicitly target pane 0
    tmux(&["split-window", "-t", &session.name, "-h"]);
    thread::sleep(Duration::from_millis(200));

    // Target pane 0 (the xolmis pane) explicitly
    let pane_target = format!("{}:0.0", session.name);

    // Get initial size
    tmux(&["send-keys", "-t", &pane_target, "tput cols", "Enter"]);
    thread::sleep(Duration::from_millis(300));
    let content1 = tmux_capture(&pane_target);

    // Parse initial cols
    let initial_cols: u32 = content1
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .next()
        .unwrap_or(0);

    // Resize pane 0 - make it larger
    let new_cols: u32 = 120;
    tmux(&[
        "resize-pane",
        "-t",
        &pane_target,
        "-x",
        &new_cols.to_string(),
    ]);
    thread::sleep(Duration::from_millis(300));

    // Check size after pane resize
    tmux(&["send-keys", "-t", &pane_target, "tput cols", "Enter"]);
    thread::sleep(Duration::from_millis(300));
    let content2 = tmux_capture(&pane_target);

    // Parse new cols (get the last number - most recent tput output)
    let final_cols: u32 = content2
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .last()
        .unwrap_or(0);

    assert!(
        final_cols == new_cols,
        "resize-pane not propagated to PTY.\nExpected: {} cols\nInitial: {} cols\nFinal: {} cols\nContent:\n{}",
        new_cols,
        initial_cols,
        final_cols,
        content2
    );
}
