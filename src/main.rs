use pty_process::blocking::{Command, Pty, Pts};
use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{ExitStatus, Child};
use std::fs::File;
use std::thread;

fn main() -> io::Result<()> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    println!(
        "Starting xolmis: Spawning '{}' in a PTY...",
        shell
    );

    let mut cmd = Command::new(&shell);
    let (pty, pts): (Pty, Pts) = pty_process::blocking::open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open PTY: {}", e)))?;

    let mut child: Child = cmd.spawn(pts)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to spawn process: {}", e)))?;

    println!("PTY spawned successfully.");

    let pty_fd = pty.as_raw_fd();

    let pty_reader_file = unsafe { File::from_raw_fd(pty_fd) };
    let pty_writer_file = unsafe { File::from_raw_fd(pty_fd) };

    let output_thread = thread::spawn(move || {
        let mut pty_out = pty_reader_file;
        let mut buffer = [0; 2048];

        loop {
            match pty_out.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let output_bytes = &buffer[..n];
                    let mut output_str = String::from_utf8_lossy(output_bytes).to_string();
                    output_str = output_str.replace("hello", "hellox");

                    let mut stdout = io::stdout().lock();
                    if let Err(e) = stdout.write_all(output_str.as_bytes()) {
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
    std::process::exit(status.code().unwrap_or(1));
}
