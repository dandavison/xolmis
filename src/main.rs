// ## xolmis: Terminal Output Transformer
//
// Problem:
// We want to intercept the output stream of an interactive shell session (like zsh)
// *before* it reaches the terminal emulator. This allows us to parse the stream,
// identify specific text patterns (e.g., file paths with line numbers), and
// embed terminal hyperlinks (using OSC 8 escape sequences) around those patterns.
// The goal is to enhance terminal output with clickable links without modifying
// the underlying shell or the programs it runs.
//
// High-Level Strategy:
// 1. Pseudo-Terminal (PTY): Instead of running the user's shell directly, xolmis
//    creates a pseudo-terminal pair (master/slave). The user's *actual* shell
//    (e.g., zsh) is launched as a child process connected to the PTY *slave* end.
// 2. Terminal Raw Mode: xolmis sets the *real* terminal (its own stdin/stdout)
//    to "raw" mode. This prevents the OS's terminal driver from interpreting most
//    control characters (like Ctrl+C, arrow keys) and line buffering, ensuring that
//    keypresses are passed directly to xolmis, and subsequently to the shell via
//    the PTY, allowing the shell's line editor (like zsh's ZLE) to function correctly.
// 3. I/O Forwarding: xolmis spawns two threads:
//    - Input Thread: Reads raw bytes from the real terminal's stdin and writes
//      them directly to the PTY master, sending user input to the shell.
//    - Output Thread: Reads potentially fragmented raw bytes from the PTY master,
//      uses a streaming UTF-8 decoder to produce valid String chunks, applies
//      transformation rules (hyperlinking) to these strings, and writes the
//      result to the real terminal's stdout.
// 4. Transformation Module: The specific rules for identifying patterns and creating
//    hyperlinks are delegated to a separate `transform` module, keeping the main
//    binary focused on PTY and terminal management.
// 5. ANSI Awareness: The transformation logic uses an ANSI parser (from delta) to
//    correctly handle existing escape codes (like colors) in the shell's output,
//    inserting hyperlinks without corrupting the formatting.
// 6. Exit Handling: Uses `std::process::exit` for simplicity, although this prevents
//    clean terminal state restoration (a deferred known issue).

use pty_process::blocking::{Command, Pty, Pts};
use pty_process::Size;
use std::env;
use std::io::{self, Read, Write, IsTerminal};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, FromRawFd};
use std::process::{ExitStatus, Child};
use std::fs::File;
use std::thread;
use terminal_size::{terminal_size, Height, Width};

// Import termios functions and flags from nix for terminal control.
// The `nix` crate provides safe wrappers around low-level Unix APIs.
use nix::sys::termios::{self, Termios, InputFlags, OutputFlags, LocalFlags, ControlFlags};

// Declare the modules responsible for transformations and ANSI parsing.
mod transform;
mod ansi;

// Imports for streaming UTF-8 decoding.
use encoding_rs::UTF_8;
use encoding_rs_io::DecodeReaderBytesBuilder;

// A helper struct using the RAII (Resource Acquisition Is Initialization) pattern.
// Its sole purpose is to store the original terminal settings and restore them
// automatically when the struct goes out of scope (i.e., when `main` exits).
// This is crucial for leaving the user's terminal in a usable state.
// Note: The current `std::process::exit` call bypasses this Drop implementation.
struct TermRestore<'a> {
    original_termios: Termios,
    fd: BorrowedFd<'a>, // Uses a borrowed FD for safety with nix API.
}

// The Drop trait defines cleanup logic that runs when a value goes out of scope.
impl<'a> Drop for TermRestore<'a> {
    fn drop(&mut self) {
        // This message indicates the drop is happening, but might interfere with
        // the terminal state if printed while still in raw mode.
        println!("Restoring terminal settings...");
        // Attempt to restore the original terminal settings using `tcsetattr`.
        // TCSANOW applies the changes immediately.
        if let Err(e) = termios::tcsetattr(self.fd, termios::SetArg::TCSANOW, &self.original_termios) {
            // Report error if restoration fails, but don't panic.
            eprintln!("Failed to restore terminal settings: {}", e);
        }
    }
}

fn main() -> io::Result<()> {
    // Get the current working directory. This is needed by the transformation logic
    // to resolve relative file paths found in the shell output.
    let cwd = env::current_dir()?;

    // Standard input (stdin) is the primary way xolmis interacts with the real terminal
    // for receiving user keypresses.
    let stdin = io::stdin();

    // We need to manipulate terminal settings (like setting raw mode). These operations
    // only make sense on an actual terminal device (TTY).
    // `is_terminal()` checks if stdin is connected to a TTY.
    // If xolmis were run with stdin piped from a file, this check would fail.
    if !stdin.is_terminal() {
        eprintln!("Error: Standard input is not a TTY.");
        return Err(io::Error::new(io::ErrorKind::Other, "Stdin not a TTY"));
    }

    // Get a safe wrapper around the raw file descriptor for stdin.
    // `BorrowedFd` ensures we don't accidentally close stdin and works with `nix`.
    let stdin_fd = stdin.as_fd();

    // Retrieve the current terminal attributes (settings) for stdin.
    // These include flags controlling input/output processing, echoing, signal handling, etc.
    let original_termios = termios::tcgetattr(stdin_fd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to get terminal attributes: {}", e)))?;

    // Instantiate the TermRestore struct. It clones the original settings.
    // `_term_restore` going out of scope at the end of `main` triggers its `drop` method
    // (unless `process::exit` is called).
    let _term_restore = TermRestore { original_termios: original_termios.clone(), fd: stdin_fd };

    // Clone the settings again to create a modified version for raw mode.
    let mut raw_termios = original_termios.clone();

    // Configure and apply raw mode settings.
    // Raw mode disables most of the kernel's terminal processing:
    // - No line buffering: Characters are available to read immediately.
    // - No echoing: Typed characters aren't automatically printed to the screen.
    // - No signal processing: Ctrl+C, Ctrl+Z etc. are sent as literal bytes.
    // - No special character handling: Backspace, delete etc. sent as literal bytes.
    // - No flow control (IXON/IXOFF).
    // This allows the application (xolmis, and subsequently the shell inside the PTY)
    // to handle all input interpretation and output formatting itself.
    // This is essential for interactive shells; without raw mode, the OS terminal
    // driver buffers lines and interprets control characters (like arrow keys,
    // backspace, Ctrl+C). This prevents the shell's own line editor (ZLE) and
    // key bindings from working correctly, as it wouldn't receive the raw key events.
    raw_termios.input_flags &= !(InputFlags::IGNBRK | InputFlags::BRKINT | InputFlags::PARMRK | InputFlags::ISTRIP | InputFlags::INLCR | InputFlags::IGNCR | InputFlags::ICRNL | InputFlags::IXON);
    raw_termios.output_flags &= !(OutputFlags::OPOST);
    raw_termios.local_flags &= !(LocalFlags::ECHO | LocalFlags::ECHONL | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN);
    raw_termios.control_flags &= !(ControlFlags::CSIZE | ControlFlags::PARENB);
    raw_termios.control_flags |= ControlFlags::CS8;
    // `cfmakeraw` is a convenience function in `nix` that sets common raw mode flags.
    termios::cfmakeraw(&mut raw_termios);
    // Note: `cfmakeraw` might not be sufficient on all platforms or for all needs.
    // More specific flag manipulation might be required for full terminal emulation.
    // VMIN/VTIME settings might also be relevant but are often handled by cfmakeraw.

    // Apply the raw mode settings to the terminal.
    // println!("Applying raw mode terminal settings..."); // Can be noisy
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &raw_termios)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to set raw terminal attributes: {}", e)))?;

    // Get the initial size (rows, columns) of the real terminal.
    // This is crucial for setting the corresponding size of the PTY.
    let term_size = terminal_size();

    // Determine the user's default shell.
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()); // Using specific shell for now
    // println!("Starting xolmis: Spawning '{}' in a PTY...", shell); // Can be noisy

    // Prepare the command to run the user's shell.
    let cmd = Command::new(&shell);

    // Create the pseudo-terminal (PTY) pair.
    // `pty_process::blocking::open()` returns:
    // - `pty`: The master end. xolmis reads shell output from and writes user input to this.
    // - `pts`: The slave end. This is passed to the child process (the shell) to use
    //          as its controlling terminal (its stdin, stdout, stderr).
    let (pty, pts): (Pty, Pts) = pty_process::blocking::open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open PTY: {}", e)))?;

    // Set initial PTY size.
    // Programs running inside the PTY (like fzf, editors) query the PTY's size
    // to render their UI. If not explicitly set, the PTY might have incorrect default
    // dimensions, causing rendering errors or crashes (e.g., fzf panic).
    if let Some((Width(cols), Height(rows))) = term_size {
        // println!("Resizing PTY to {}x{}", cols, rows); // Can be noisy
        let pty_size = Size::new(rows, cols);
        // The `resize` method borrows `pty` mutably here, which is allowed even if
        // the `pty` binding isn't `mut`.
        pty.resize(pty_size)
           .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to resize PTY: {}", e)))?;
    } else {
        eprintln!("Warning: Could not get terminal size. PTY might have incorrect dimensions.");
    }
    // Note: Handling terminal resize *while running* (SIGWINCH signal) is not yet implemented.
    // --- End PTY size setup ---

    // Spawn the user's shell as a child process.
    // Critically, the shell is attached to the PTY slave end (`pts`).
    // `cmd.spawn()` takes ownership of `pts`.
    let mut child: Child = cmd.spawn(pts)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to spawn process: {}", e)))?;

    // println!("PTY spawned successfully."); // Can be noisy

    // --- File Descriptor Handling for Threads ---
    // We need to read from and write to the PTY master (`pty`) from separate threads.
    // The `Pty` object owns the master file descriptor.
    // We cannot simply clone `Pty` as it doesn't implement `Clone`.
    // Using `Arc<Mutex<Pty>>` caused deadlocks previously due to blocking reads holding the lock.
    // The safe `Arc<Mutex<Pty>>` approach caused deadlocks due to blocking reads
    // holding the lock needed by the writer. Non-blocking I/O attempts failed due to
    // build issues. This unsafe approach avoids the deadlock but causes an IO safety
    // violation (double-close attempt) on exit, which is currently accepted.
    // Therefore, we resort to `unsafe` code to duplicate the file descriptor.
    // This gives each thread its own `File` handle pointing to the same underlying PTY master.
    // **WARNING:** This is fundamentally unsafe because `File::from_raw_fd` takes ownership.
    // Both `File` objects now believe they own the FD and will try to close it on Drop.
    // This leads to a double-close attempt, causing the "IO Safety violation" runtime error
    // or panic when the second file object is dropped (usually when xolmis exits).
    // A robust solution would involve non-blocking I/O with `Arc<Mutex<Pty>>` or async I/O.
    // This unsafe approach is kept *temporarily* because it avoids the deadlock.
    let pty_fd = pty.as_raw_fd();
    let pty_reader_file = unsafe { File::from_raw_fd(pty_fd) };
    let pty_writer_file = unsafe { File::from_raw_fd(pty_fd) };
    // The original `pty` object is now effectively just holding the FD open until the
    // unsafe `File` handles are done. It's not used directly after this.
    // --- End File Descriptor Handling ---

    // Clone the current working directory for the output thread.
    let thread_cwd = cwd.clone();

    // --- Output Thread --- 
    // Reads output from the shell (via PTY master), decodes UTF-8, transforms it,
    // and writes to real stdout.
    let output_thread = thread::spawn(move || {
        // Wrap the PTY reader file handle with a streaming UTF-8 decoder.
        // Simple `read` calls can split multi-byte UTF-8 characters across buffer
        // boundaries. Processing these chunks individually with `String::from_utf8_lossy`
        // previously caused `` replacement characters to appear incorrectly in the output.
        // This streaming decoder correctly handles state across reads.
        let mut decoder = DecodeReaderBytesBuilder::new()
            .encoding(Some(UTF_8))
            .build(pty_reader_file);

        // Buffer for the decoded bytes.
        let mut byte_buffer = [0; 2048];

        loop {
            // Read the next chunk of *decoded* bytes from the PTY stream.
            // The `decoder` handles buffering incomplete sequences internally.
            match decoder.read(&mut byte_buffer) {
                Ok(0) => break, // EOF: Shell process exited.
                Ok(n) => {
                    // The bytes in `byte_buffer[..n]` are guaranteed by the decoder
                    // to be valid UTF-8 (or contain replacement chars if the *source*
                    // stream was truly invalid, which is unlikely for shell output).
                    let decoded_bytes = &byte_buffer[..n];

                    // Convert the guaranteed-valid UTF-8 bytes to a string slice.
                    // This `from_utf8` call should ideally never fail here.
                    match std::str::from_utf8(decoded_bytes) {
                        Ok(decoded_str) => {
                            // Pass the valid string chunk to the transformation logic.
                            let transformed_str = transform::transform(decoded_str, &thread_cwd);

                            // Write the (potentially transformed) result to the real terminal stdout.
                            let mut stdout = io::stdout().lock();
                            if let Err(e) = stdout.write_all(transformed_str.as_bytes()) {
                                eprintln!("Error writing to stdout: {}", e);
                                break; // Stop if we can't write to stdout.
                            }
                            // Flush stdout to ensure output appears immediately.
                            let _ = stdout.flush();
                        }
                        Err(e) => {
                             // Log if unexpected UTF-8 error occurs after decoding.
                             eprintln!("UTF-8 conversion error after decode: {}. Skipping chunk.", e);
                        }
                    }
                }
                Err(e) => {
                    // Retry if the read was interrupted by a signal.
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    // Handle other read/decode errors.
                    eprintln!("Error reading/decoding from PTY: {}", e);
                    break;
                }
            }
        }
    });
    // --- End Output Thread ---

    // --- Input Thread ---
    // Reads input from the real terminal stdin, writes it to the PTY master (shell).
    let input_thread = thread::spawn(move || {
        // The `pty_writer_file` handle points to the PTY master FD.
        let mut pty_writer = pty_writer_file;
        // Lock stdin for efficient reading.
        let mut stdin = io::stdin().lock();
        // Buffer for stdin data.
        let mut buffer = [0; 1024];

        loop {
            // Read user input from the real terminal (blocks until input).
            match stdin.read(&mut buffer) {
                Ok(0) => break, // EOF: Real terminal stdin closed.
                Ok(n) => {
                    // Write the received bytes directly to the PTY master, sending to the shell.
                    if let Err(e) = pty_writer.write_all(&buffer[..n]) {
                        eprintln!("Error writing to PTY: {}", e);
                        break; // Stop if we can't write to the PTY.
                    }
                    // Flush the PTY writer buffer.
                    let _ = pty_writer.flush();
                }
                Err(e) => {
                    // Retry if interrupted by a signal.
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    // Handle other stdin read errors.
                    eprintln!("Error reading from stdin: {}", e);
                    break;
                 }
            }
        }
    });
    // --- End Input Thread ---

    // --- Main Thread Waits ---
    // Wait for the child shell process to terminate.
    // This blocks the main thread.
    let status: ExitStatus = match child.wait() {
        Ok(status) => {
            // Log the exit status.
            println!("\nShell process exited with status: {}", status);
            status
        }
        Err(e) => {
            eprintln!("Failed to wait for child process: {}", e);
            // If waiting fails, return an error. This would trigger terminal restore
            // if main returned Result, but `process::exit` bypasses it.
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to wait for child"));
        }
    };

    // Wait for the I/O threads to complete their work.
    // If the shell exits, output_thread should finish soon after detecting EOF.
    // input_thread might block indefinitely if stdin isn't closed (e.g., user doesn't Ctrl+D)
    // which would cause a hang here if not for the process::exit below.
    output_thread.join().expect("Output thread panicked");
    input_thread.join().expect("Input thread panicked");

    // --- Exit --- 
    println!("xolmis finished.");
    // Use immediate exit via std::process::exit.
    // Letting `main` return naturally caused hangs because `input_thread.join()`
    // would block indefinitely waiting on `stdin.read()` after the child shell had exited.
    // Using `exit` terminates all threads abruptly, avoiding the hang.
    // Known Issue: This prevents the `_term_restore` destructor from running,
    // leaving the terminal in raw mode.
    std::process::exit(status.code().unwrap_or(1));
    // --- End Exit ---
}
