use std::process::Command;
use std::thread;
use std::time::Duration;

const SESSION: &str = "xolmis_test";

fn tmux(args: &[&str]) -> std::process::Output {
    Command::new("tmux")
        .args(args)
        .output()
        .expect("tmux command failed")
}

fn capture_pane() -> String {
    let output = tmux(&["capture-pane", "-t", SESSION, "-p"]);
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn setup() {
    // Kill any existing test session
    let _ = tmux(&["kill-session", "-t", SESSION]);

    // Build xolmis
    let status = Command::new("cargo")
        .args(["build"])
        .status()
        .expect("cargo build failed");
    assert!(status.success(), "cargo build failed");

    // Start tmux with xolmis
    let status = tmux(&[
        "new-session",
        "-d",
        "-s",
        SESSION,
        "-x",
        "80",
        "-y",
        "24",
        "./target/debug/xolmis",
    ])
    .status;
    assert!(status.success(), "failed to start tmux session");

    // Wait for shell to initialize
    thread::sleep(Duration::from_millis(500));
}

fn teardown() {
    let _ = tmux(&["kill-session", "-t", SESSION]);
}

#[test]
fn test_hello_world() {
    setup();

    // Send a simple echo command
    tmux(&[
        "send-keys",
        "-t",
        SESSION,
        "echo hello_from_xolmis",
        "Enter",
    ]);
    thread::sleep(Duration::from_millis(200));

    let content = capture_pane();
    teardown();

    assert!(
        content.contains("hello_from_xolmis"),
        "expected 'hello_from_xolmis' in pane content:\n{}",
        content
    );
}
