use pty_process::blocking::{Command, Pty, Pts};
use std::env;
use std::io::{self, Read, Write, IsTerminal};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Child};
use std::fs::{File, OpenOptions};
use std::thread;

// Import termios functions and flags from nix
use nix::sys::termios::{self, Termios, InputFlags, OutputFlags, LocalFlags, ControlFlags};

// Import Regex
use regex::Regex;

// Helper struct to restore terminal settings on drop
struct TermRestore<'a> {
    original_termios: Termios,
    fd: BorrowedFd<'a>,
}

impl<'a> Drop for TermRestore<'a> {
    fn drop(&mut self) {
        println!("Restoring terminal settings...");
        if let Err(e) = termios::tcsetattr(self.fd, termios::SetArg::TCSANOW, &self.original_termios) {
            eprintln!("Failed to restore terminal settings: {}", e);
        }
    }
}

// --- Helper functions for transformation --- (Adapted from original)
fn transform(line: &str, cwd: &Path, re: &Regex) -> String {
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
// --- End Helper functions ---

fn main() -> io::Result<()> {
    // Get CWD once
    let cwd = env::current_dir()?;

    // Get stdin
    let stdin = io::stdin();

    // Check if stdin is a TTY using the IsTerminal trait
    if !stdin.is_terminal() {
        eprintln!("Error: Standard input is not a TTY.");
        return Err(io::Error::new(io::ErrorKind::Other, "Stdin not a TTY"));
    }

    // Get BorrowedFd for stdin
    let stdin_fd = stdin.as_fd();

    // Get original terminal attributes using BorrowedFd
    let original_termios = termios::tcgetattr(stdin_fd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to get terminal attributes: {}", e)))?;

    // Create a scope guard to restore terminal settings on exit
    let _term_restore = TermRestore { original_termios: original_termios.clone(), fd: stdin_fd };

    // Create raw mode attributes
    let mut raw_termios = original_termios.clone();
    // Disable echo, canonical mode (line buffering), signal chars (Ctrl+C), flow control
    raw_termios.input_flags &= !(InputFlags::IGNBRK | InputFlags::BRKINT | InputFlags::PARMRK | InputFlags::ISTRIP | InputFlags::INLCR | InputFlags::IGNCR | InputFlags::ICRNL | InputFlags::IXON);
    raw_termios.output_flags &= !(OutputFlags::OPOST);
    raw_termios.local_flags &= !(LocalFlags::ECHO | LocalFlags::ECHONL | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN);
    raw_termios.control_flags &= !(ControlFlags::CSIZE | ControlFlags::PARENB);
    raw_termios.control_flags |= ControlFlags::CS8;
    // Set VMIN = 1, VTIME = 0 (read returns after 1 byte is available, no timeout)
    termios::cfmakeraw(&mut raw_termios);

    // Apply raw mode settings using BorrowedFd
    println!("Applying raw mode terminal settings...");
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &raw_termios)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to set raw terminal attributes: {}", e)))?;

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    println!(
        "Starting xolmis: Spawning '{}' in a PTY...",
        shell
    );

    let cmd = Command::new(&shell);
    let (pty, pts): (Pty, Pts) = pty_process::blocking::open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open PTY: {}", e)))?;

    let mut child: Child = cmd.spawn(pts)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to spawn process: {}", e)))?;

    println!("PTY spawned successfully.");

    let pty_fd = pty.as_raw_fd();

    let pty_reader_file = unsafe { File::from_raw_fd(pty_fd) };
    let pty_writer_file = unsafe { File::from_raw_fd(pty_fd) };

    // Clone cwd for the thread
    let thread_cwd = cwd.clone();

    let output_thread = thread::spawn(move || {
        // Create the Regex once
        let re = Regex::new(r"^([a-zA-Z-_./]+):(\d+)").unwrap(); // Adjust regex as needed

        let mut pty_out = pty_reader_file;
        let mut buffer = [0; 2048];

        loop {
            match pty_out.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let output_bytes = &buffer[..n];
                    let mut lossy_str = String::from_utf8_lossy(output_bytes);

                    // Apply transformation using the helper function
                    let transformed_str = transform(&lossy_str, &thread_cwd, &re);

                    let mut stdout = io::stdout().lock();
                    if let Err(e) = stdout.write_all(transformed_str.as_bytes()) {
                        eprintln!("Error writing to stdout: {}", e);
                        break;
                    }
                    let _ = stdout.flush();
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    eprintln!("Error reading from PTY: {}", e);
                    break;
                }
            }
        }
    });

    let input_thread = thread::spawn(move || {
        let mut pty_writer = pty_writer_file;
        let mut stdin = io::stdin().lock();
        let mut buffer = [0; 1024];

        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => {
                    break;
                }
                Ok(n) => {
                    if let Err(e) = pty_writer.write_all(&buffer[..n]) {
                        eprintln!("Error writing to PTY: {}", e);
                        break;
                    }
                    let _ = pty_writer.flush();
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    eprintln!("Error reading from stdin: {}", e);
                    break;
                }
            }
        }
    });

    let status: ExitStatus = match child.wait() {
        Ok(status) => {
            println!("\nShell process exited with status: {}", status);
            status
        }
        Err(e) => {
            eprintln!("Failed to wait for child process: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to wait for child"));
        }
    };

    output_thread.join().expect("Output thread panicked");
    input_thread.join().expect("Input thread panicked");

    println!("xolmis finished.");
    // Explicit exit is fine, _term_restore handles cleanup
    std::process::exit(status.code().unwrap_or(1));
}
