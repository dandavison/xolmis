<p align="center">
  <img src="https://cdn.download.ams.birds.cornell.edu/api/v1/asset/587510951/1200" alt="Image" width="400">
  <br>
  <span style="font-size: small; color: gray;"><i>Xolmis velatus</i> (White-rumped Monjita)</span>
</p>

# xolmis: Terminal Output Transformer


## Problem

Often, the output of shell commands contains text patterns (like `path/to/file:123`) that would be useful as clickable hyperlinks within the terminal. Manually copying these or configuring individual tools to emit links can be cumbersome. We want a way to automatically identify these patterns in the general output stream of an interactive shell session (like zsh) *before* it reaches the terminal emulator and embed hyperlinks (using OSC 8 escape sequences) around them, without modifying the underlying shell or the programs it runs.

## High-Level Strategy

xolmis acts as a wrapper around your interactive shell, intercepting its input and output to inject these hyperlinks dynamically.

1.  **Pseudo-Terminal (PTY):** Instead of running your shell directly, xolmis creates a pseudo-terminal pair (master/slave). Your actual shell (e.g., zsh) is launched as a child process connected to the PTY *slave* end. xolmis interacts with the *master* end.
2.  **Terminal Raw Mode:** xolmis sets the *real* terminal (its own stdin/stdout) to "raw" mode. This ensures that most control sequences (arrow keys, Ctrl+C, etc.) are passed through directly to the wrapped shell, allowing shell features like line editing (ZLE) and key bindings to function correctly.
3.  **I/O Forwarding & Transformation:** xolmis uses threads to handle I/O:
    *   An **input thread** reads raw bytes from the real terminal's stdin and forwards them to the PTY master (sending input to the shell).
    *   An **output thread** reads raw bytes from the PTY master (output from the shell), decodes them using a streaming UTF-8 decoder (to handle multi-byte characters split across reads), applies transformation rules to identify and hyperlink patterns within the resulting text, and writes the final output (with embedded hyperlinks) to the real terminal's stdout.
4.  **Transformation Module:** The specific rules for pattern matching and hyperlink generation reside in the `src/transform.rs` module.
5.  **ANSI Awareness:** The transformation logic uses an ANSI parser (logic derived from the `delta` tool) to iterate through text segments and ANSI escape codes separately. This allows hyperlinks to be inserted around text *without* breaking existing formatting like colors.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              TERMINAL EMULATOR (iTerm2, WezTerm, etc.)              │
│                                                                                     │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                              USER'S VIEW                                    │   │
│   │                                                                             │   │
│   │   $ grep -n TODO src/*.rs                                                   │   │
│   │   src/main.rs:42:    // TODO: handle resize                                 │   │
│   │                ▲                                                            │   │
│   │                └── clickable hyperlink! (cursor://file/...src/main.rs:42)   │   │
│   │                                                                             │   │
│   └─────────────────────────────────────────────────────────────────────────────┘   │
│                                       ▲                                             │
│                    transformed output │                                             │
│                    (with OSC 8 links) │                                             │
└───────────────────────────────────────┼─────────────────────────────────────────────┘
                                        │
┌───────────────────────────────────────┴─────────────────────────────────────────────┐
│                                    XOLMIS                                           │
│                                                                                     │
│   ┌─────────────┐                                     ┌─────────────────────────┐   │
│   │ REAL STDIN  │                                     │      REAL STDOUT        │   │
│   │ (raw mode)  │                                     │                         │   │
│   └──────┬──────┘                                     └────────────▲────────────┘   │
│          │                                                         │                │
│          │ keystrokes                              transformed text│                │
│          │                                                         │                │
│   ┌──────▼──────────────────────────────────────────────────────────────────────┐   │
│   │                          MAIN THREAD                                        │   │
│   │                                                                             │   │
│   │  • Sets terminal to RAW MODE (disable echo, buffering, signals)             │   │
│   │  • Creates PTY master/slave pair                                            │   │
│   │  • Spawns child shell attached to PTY slave                                 │   │
│   │  • Waits for child process to exit                                          │   │
│   │                                                                             │   │
│   └──────┬─────────────────────────────────────────────────────────┬────────────┘   │
│          │                                                         │                │
│   ┌──────▼──────┐                                       ┌──────────┴────────────┐   │
│   │INPUT THREAD │                                       │    OUTPUT THREAD      │   │
│   │             │                                       │                       │   │
│   │ Read stdin  │                                       │  ┌─────────────────┐  │   │
│   │      │      │                                       │  │ UTF-8 Decoder   │  │   │
│   │      ▼      │                                       │  │ (streaming)     │  │   │
│   │ Write to    │                                       │  └────────┬────────┘  │   │
│   │ PTY master  │                                       │           ▼           │   │
│   │             │                                       │  ┌─────────────────┐  │   │
│   └──────┬──────┘                                       │  │   TRANSFORM     │  │   │
│          │                                              │  │                 │  │   │
│          │                                              │  │ Strip ANSI ───┐ │  │   │
│          │                                              │  │               │ │  │   │
│          │                                              │  │ Match regex ◄─┘ │  │   │
│          │                                              │  │ (path:line)     │  │   │
│          │                                              │  │               │ │  │   │
│          │                                              │  │ Check file    │ │  │   │
│          │                                              │  │ exists ◄──────┘ │  │   │
│          │                                              │  │               │ │  │   │
│          │                                              │  │ Inject OSC 8◄─┘ │  │   │
│          │                                              │  │ hyperlinks      │  │   │
│          │                                              │  │               │ │  │   │
│          │                                              │  │ Restore ANSI◄─┘ │  │   │
│          │                                              │  │ formatting      │  │   │
│          │                                              │  └────────┬────────┘  │   │
│          │                                              │           │           │   │
│          │                                              │  Write to stdout      │   │
│          │                                              └───────────────────────┘   │
│          │                                                         ▲                │
│          │                                                         │                │
│   ┌──────▼─────────────────────────────────────────────────────────┴────────────┐   │
│   │                              PTY MASTER                                     │   │
│   └─────────────────────────────────┬───────────────────────────────────────────┘   │
│                                     │                                               │
└─────────────────────────────────────┼───────────────────────────────────────────────┘
                                      │
┌─────────────────────────────────────▼───────────────────────────────────────────────┐
│                                 PTY SLAVE                                           │
│                          (connected to shell's stdin/stdout/stderr)                 │
│                                                                                     │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                        CHILD SHELL (zsh, bash, etc.)                        │   │
│   │                                                                             │   │
│   │   • Runs interactively with line editing (ZLE/readline)                     │   │
│   │   • Executes commands (grep, cargo, python, etc.)                           │   │
│   │   • Produces output with paths like "src/main.rs:42"                        │   │
│   │                                                                             │   │
│   └─────────────────────────────────────────────────────────────────────────────┘   │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘

                               ┌─────────────────────────────┐
                               │       TRANSFORMATION        │
                               │         PIPELINE            │
                               └──────────────┬──────────────┘
                                              │
                                              ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│  INPUT: "\x1b[31msrc/main.rs:42\x1b[0m: TODO"                                       │
│                                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────┐    │
│  │ 1. ANSI PARSING (src/ansi/)                                                 │    │
│  │    AnsiElementIterator parses stream into:                                  │    │
│  │    [Sgr(Red), Text("src/main.rs:42"), Sgr(Reset), Text(": TODO")]           │    │
│  │                                                                             │    │
│  │ 2. STRIP FOR MATCHING                                                       │    │
│  │    Stripped text: "src/main.rs:42: TODO"                                    │    │
│  │                                                                             │    │
│  │ 3. REGEX MATCHING (src/rules.rs)                                            │    │
│  │    Rules: FilePath, PythonTraceback, IpdbTraceback                          │    │
│  │    Match: "src/main.rs:42" → path="src/main.rs", line=42                    │    │
│  │                                                                             │    │
│  │ 4. PATH VALIDATION                                                          │    │
│  │    Resolve: cwd + "src/main.rs" → /full/path/src/main.rs                    │    │
│  │    Check: file exists? ✓                                                    │    │
│  │                                                                             │    │
│  │ 5. HYPERLINK INJECTION                                                      │    │
│  │    URL: cursor://file//full/path/src/main.rs:42                             │    │
│  │    OSC 8: \x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\                           │    │
│  │                                                                             │    │
│  │ 6. ANSI PRESERVATION                                                        │    │
│  │    Re-inject original ANSI codes around the hyperlink                       │    │
│  └─────────────────────────────────────────────────────────────────────────────┘    │
│                                                                                     │
│  OUTPUT: "\x1b[31m\x1b]8;;cursor://file//.../main.rs:42\x1b\\src/main.rs:42"        │
│          "\x1b]8;;\x1b\\\x1b[0m: TODO"                                              │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘

                               ┌─────────────────────────────┐
                               │       MODULE STRUCTURE      │
                               └──────────────┬──────────────┘
                                              │
                                              ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│  src/                                                                               │
│  ├── main.rs          PTY creation, raw mode, I/O threads, process management       │
│  ├── transform.rs     Core transformation: match finding, hyperlink generation      │
│  ├── rules.rs         Regex patterns: FilePath, PythonTraceback, IpdbTraceback      │
│  └── ansi/                                                                          │
│      ├── mod.rs       ANSI utilities: strip_ansi_codes, ansi_preserving_index       │
│      └── iterator.rs  AnsiElementIterator: state-machine ANSI parser                │
│                                                                                     │
│  Key Dependencies:                                                                  │
│  • pty-process     - PTY creation and process spawning                              │
│  • nix             - Terminal control (raw mode via termios)                        │
│  • encoding_rs     - Streaming UTF-8 decoding                                       │
│  • regex           - Pattern matching for file paths                                │
│  • anstyle-parse   - Low-level ANSI escape sequence parsing                         │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Current State & Known Issues

*   **Functionality:** Wraps a shell, handles raw mode, performs basic `path:line` hyperlinking using OSC 8 sequences compatible with many modern terminals (like WezTerm, iTerm2, Alacritty). Correctly handles UTF-8 decoding and preserves ANSI colors during transformation.
*   **Terminal State on Exit:** Uses `std::process::exit()` for termination to avoid potential hangs. **Known Issue:** This prevents terminal settings from being properly restored, potentially leaving your terminal in a bad state after `xolmis` exits. Running `reset` in the parent shell usually fixes this.
*   **Unsafe FD Handling:** Uses `unsafe File::from_raw_fd` to share the PTY master between threads due to previous deadlocks with safer methods. **Known Issue:** This causes an "IO Safety violation" error message or panic on exit due to a double-close attempt on the file descriptor.
*   **Resizing:** Only sets initial PTY size. Does not handle terminal resizing while running (`SIGWINCH`). Resizing the window while TUI applications like `fzf` are running inside `xolmis` may cause display errors.

## Usage (Development)

1.  **Build:**
    ```bash
    cargo build --release
    ```
2.  **Run:** Launch `xolmis` directly from your normal shell session (running inside tmux or your preferred terminal):
    ```bash
    ./target/release/xolmis
    ```
3.  **Interact:** Use the wrapped shell session as normal. Output matching the rules in `src/transform.rs` (currently `path:line` patterns) should appear as hyperlinks.
4.  **Reset Terminal (if needed):** If your original terminal prompt looks strange after exiting `xolmis`, run:
    ```bash
    reset
    ```

## Future Integration (Example)

Once stable, instead of running manually, you could add logic to your shell's startup file (e.g., `~/.zshrc`) to automatically wrap your sessions:

```bash
# Add as the last line of your shell config
if [ -z "$XOLMIS" ]; then
    export XOLMIS="true"
    exec /PATH/TO/xolmis
fi
```
(Note: While the terminal restoration issue mainly affects running `xolmis` as a child process during development, the unsafe file descriptor handling issue should ideally be addressed for robust `exec` integration).