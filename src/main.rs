use pty_process::blocking::{Command, Pty, Pts};
use pty_process::Size;
use std::env;
use std::io::{self, Read, Write, IsTerminal};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, FromRawFd};
use std::process::{ExitStatus, Child};
use std::fs::File;
use std::thread;
use terminal_size::{terminal_size, Height, Width};

use nix::sys::termios::{self, Termios, InputFlags, OutputFlags, LocalFlags, ControlFlags};

mod transform;
mod ansi;

use encoding_rs::UTF_8;
use encoding_rs_io::DecodeReaderBytesBuilder;

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

fn main() -> io::Result<()> {
    let cwd = env::current_dir()?;
    let stdin = io::stdin();

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
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &raw_termios)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to set raw terminal attributes: {}", e)))?;

    let term_size = terminal_size();

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let cmd = Command::new(&shell);
    let (pty, pts): (Pty, Pts) = pty_process::blocking::open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open PTY: {}", e)))?;

    if let Some((Width(cols), Height(rows))) = term_size {
        let pty_size = Size::new(rows, cols);
        pty.resize(pty_size)
           .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to resize PTY: {}", e)))?;
    } else {
        eprintln!("Warning: Could not get terminal size. PTY might have incorrect dimensions.");
    }

    let mut child: Child = cmd.spawn(pts)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to spawn process: {}", e)))?;

    let pty_fd = pty.as_raw_fd();

    let pty_reader_file = unsafe { File::from_raw_fd(pty_fd) };
    let pty_writer_file = unsafe { File::from_raw_fd(pty_fd) };

    let thread_cwd = cwd.clone();

    let output_thread = thread::spawn(move || {
        // Wrap the PTY reader with a UTF-8 decoder
        let mut decoder = DecodeReaderBytesBuilder::new()
            .encoding(Some(UTF_8))
            .build(pty_reader_file);

        // Use a byte buffer for reading decoded bytes
        let mut byte_buffer = [0; 2048];

        loop {
            // Read decoded bytes into the byte buffer
            match decoder.read(&mut byte_buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    // Bytes read are already decoded to UTF-8 (or replacements)
                    let decoded_bytes = &byte_buffer[..n];

                    // Convert the valid UTF-8 bytes to a string slice
                    match std::str::from_utf8(decoded_bytes) {
                        Ok(decoded_str) => {
                            // Pass the correctly decoded string chunk to transform
                            let transformed_str = transform::transform(decoded_str, &thread_cwd);

                            let mut stdout = io::stdout().lock();
                            if let Err(e) = stdout.write_all(transformed_str.as_bytes()) {
                                eprintln!("Error writing to stdout: {}", e);
                                break;
                            }
                            let _ = stdout.flush();
                        }
                        Err(e) => {
                             // This should theoretically not happen if encoding_rs works correctly
                             eprintln!("UTF-8 conversion error after decode: {}. Skipping chunk.", e);
                        }
                    }
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    eprintln!("Error reading/decoding from PTY: {}", e);
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
