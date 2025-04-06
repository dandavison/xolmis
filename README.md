<p align="center">
  <img src="https://cdn.download.ams.birds.cornell.edu/api/v1/asset/587510951/1200" alt="Image" width="400">
  <br>
  <span style="font-size: small; color: gray;"><i>Xolmis velatus</i> (White-rumped Monjita)</span>
</p>

# xolmis: Terminal Output Transformer

_xolmis was designed and implemented by gemini-2.5-pro-exp-03-25 using Cursor's agent mode, including code comments and
README (but excluding the [ansi](src/ansi) module)._


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