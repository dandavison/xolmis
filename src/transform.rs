use std::path::{Path, PathBuf};

// Use the updated types from the rules module
use crate::rules::{CompiledRule, get_compiled_rules};

use crate::ansi::{strip_ansi_codes, ansi_preserving_index};
use crate::ansi::iterator::{AnsiElementIterator, Element};

#[derive(Debug)]
struct MatchInfo<'a> {
    // Start and end character indices in the *stripped* text
    stripped_start: usize,
    stripped_end: usize,
    // The matched text *without* internal ANSI codes (used for whitespace check)
    #[allow(dead_code)] // Allow dead code for now, might be used later
    stripped_text: &'a str,
    path: &'a str,
    line: u32,
    #[allow(dead_code)] // Allow this field to be unused for now
    rule_name: &'static str,
}

pub fn transform(original_chunk: &str, cwd: &Path) -> String {
    // Check if the chunk contains the OSC 8 hyperlink introducer.
    // If it does, return the chunk verbatim to avoid nested links.
    if original_chunk.contains("\x1b]8;;") {
        return original_chunk.to_string();
    }
    let stripped_chunk = strip_ansi_codes(original_chunk);
    let mut output = String::with_capacity(original_chunk.len());
    let available_rules = get_compiled_rules();
    let mut matches = Vec::new();

    // Collect matches based on the stripped chunk
    for rule in available_rules {
        collect_matches(rule, &stripped_chunk, &mut matches);
    }

    // Sort matches by start index in the stripped text
    matches.sort_by_key(|m| m.stripped_start);

    let mut last_appended_original_byte_end = 0;
    let mut last_processed_stripped_end = 0;
    let original_bytes = original_chunk.as_bytes();

    for m in matches {
        // Ensure this match (in stripped text space) doesn't overlap with the previous one processed
        if m.stripped_start >= last_processed_stripped_end {
             let full_path = resolve_path(cwd, m.path);
             if full_path.exists() {
                 // Find the corresponding byte indices in the original chunk
                 if let Some((original_start, original_end)) =
                     find_original_indices(original_chunk, m.stripped_start, m.stripped_end)
                 {
                     // Ensure indices are still valid and ordered after potential adjustment
                     if original_start <= original_end && original_start >= last_appended_original_byte_end {
                         let mut link_slice_start = original_start;

                         // Check for leading newline case
                         if original_start < original_chunk.len() &&
                            original_bytes[original_start] == b'\n' &&
                            m.stripped_text.starts_with(|c: char| c.is_whitespace())
                         {
                             // Append preceding text *including* the newline
                             output.push_str(&original_chunk[last_appended_original_byte_end ..= original_start]);
                             // Start the link slice *after* the newline
                             if original_start + 1 <= original_end { // Avoid panic if end is newline too
                                 link_slice_start = original_start + 1;
                             }
                             // If start+1 > end, the slice will be empty, which is handled below
                         } else {
                             // Append preceding text *excluding* the start offset
                             output.push_str(&original_chunk[last_appended_original_byte_end .. original_start]);
                             // Start link slice at the original start
                             link_slice_start = original_start;
                         }

                         // Append the text from the original chunk since the last append point
                         // Get the original text slice, including ANSI codes
                         // Use link_slice_start which might be adjusted past a leading newline
                         // Ensure start <= end before slicing
                         if link_slice_start <= original_end {
                              let original_text_slice = &original_chunk[link_slice_start..original_end];

                              // Format and append hyperlink using the original text slice
                              let link_url = format_cursor_hyperlink(&full_path, m.line);
                              let hyperlinked_text = format_osc8_hyperlink(&link_url, original_text_slice);
                              output.push_str(&hyperlinked_text);
                         } else {
                              // Slice would be invalid (start > end), append nothing for the link part
                         }

                         last_appended_original_byte_end = original_end; // Update last append point
                         last_processed_stripped_end = m.stripped_end;   // Update last processed point in stripped text

                     } else {
                         // Adjusted indices are invalid or overlap incorrectly, skip this match for safety
                         // We might lose a link here, but it prevents panic/corruption.
                         // Consider logging this case if it happens frequently.
                         last_processed_stripped_end = m.stripped_end; // Still mark as processed
                     }
                 } else {
                     // Handle cases where original indices couldn't be found (should be rare)
                     last_processed_stripped_end = m.stripped_end; // Mark as processed
                 }
            } else {
               // Path doesn't look like a linkable path, skip linking
               last_processed_stripped_end = m.stripped_end; // Mark as processed
            }
        } // else: Overlapping match, implicitly skipped by not entering the `if`
    }

    // Append the remaining text from the original chunk after the last match
    output.push_str(&original_chunk[last_appended_original_byte_end..]);
    output
}

// Helper to find original byte indices based on stripped indices
fn find_original_indices(original_text: &str, stripped_start: usize, stripped_end: usize) -> Option<(usize, usize)> {
    let original_start_byte = ansi_preserving_index(original_text, stripped_start)?;

    let stripped_len = stripped_end - stripped_start;
    let mut current_text_len = 0;
    let mut text_content_end_byte = original_start_byte; // Byte offset where text content ends
    let mut found_text_end = stripped_len == 0; // If zero length, we are already at the end

    // 1. Find the byte offset where the text content ends
    if !found_text_end {
        for element in AnsiElementIterator::new(&original_text[original_start_byte..]) {
            if let Element::Text(_start, end) = element {
                let segment_len = end - _start;
                if current_text_len + segment_len >= stripped_len {
                    let remaining_len = stripped_len - current_text_len;
                    text_content_end_byte = original_start_byte + _start + remaining_len;
                    found_text_end = true;
                    break; // Found the end of the text content
                } else {
                    current_text_len += segment_len;
                    // Keep track of the end byte of this text segment in case it's the last before match ends
                    text_content_end_byte = original_start_byte + end;
                }
            } else {
                // If we encounter non-text before finding the text end, update potential end
                // This handles cases where the match might end *within* ANSI codes (unlikely but possible)
                let element_end = match element {
                    Element::Sgr(_, _, end) => end,
                    Element::Csi(_, end) => end,
                    Element::Esc(_, end) => end,
                    Element::Osc(_, end) => end,
                    Element::Text(_, _) => unreachable!(), // Handled above
                };
                 text_content_end_byte = original_start_byte + element_end;
            }
        }
    }

    // If we couldn't find the end of the text content (e.g., stripped indices out of bounds)
    if !found_text_end {
        return None;
    }

    // 2. Find the final end byte by consuming trailing ANSI codes
    let mut final_end_byte = text_content_end_byte;
    if text_content_end_byte < original_text.len() {
        for element in AnsiElementIterator::new(&original_text[text_content_end_byte..]) {
            match element {
                Element::Text(_, _) => break, // Stop at the next text element
                Element::Sgr(_, _, end) |
                Element::Csi(_, end) |
                Element::Esc(_, end) |
                Element::Osc(_, end) => {
                    // Update final end byte to include this ANSI code
                    final_end_byte = text_content_end_byte + end;
                }
            }
        }
    }

    // Basic sanity check
    if final_end_byte > original_text.len() {
        return None; // Should not happen if logic is correct
    }

    Some((original_start_byte, final_end_byte))
}

// Updated helper to use CompiledRule struct and populate MatchInfo correctly
fn collect_matches<'a>(
    rule: &CompiledRule,
    stripped_text_segment: &'a str,
    matches: &mut Vec<MatchInfo<'a>>,
) {
    for caps in rule.regex.captures_iter(stripped_text_segment) {
        if let (Some(match_obj), Some(path_match), Some(line_num_match)) =
            (caps.get(0), caps.get(rule.path_group_index), caps.get(rule.line_group_index))
        {
            if let Ok(line_num) = line_num_match.as_str().parse::<u32>() {
                if !path_match.as_str().is_empty() {
                    matches.push(MatchInfo {
                        stripped_start: match_obj.start(),
                        stripped_end: match_obj.end(),
                        stripped_text: match_obj.as_str(),
                        path: path_match.as_str(),
                        line: line_num,
                        rule_name: rule.name,
                    });
                }
            }
        }
    }
}

// Helper to resolve path relative to cwd or handle absolute paths
fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        // TODO: Consider handling '~' expansion if needed?
        cwd.join(path)
    }
}

// Creates a hyperlink target URL (using custom cursor:// scheme for potential editor integration)
fn format_cursor_hyperlink(absolute_path: &Path, line: u32) -> String {
    // Attempt to get a canonical path, fall back to the resolved absolute path
    let canonical_path = absolute_path.canonicalize().unwrap_or_else(|_| absolute_path.to_path_buf());
    // Use to_string_lossy to handle potential non-UTF8 paths gracefully
    format!("cursor://file/{}:{}", canonical_path.to_string_lossy(), line)
}

// Formats the text with OSC 8 terminal hyperlinks
fn format_osc8_hyperlink(url: &str, text: &str) -> String {
    format!(
        "]8;;{}\\{}]8;;\\", // Use double backslash for escape sequence in format!
        url,
        text
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    // Helper to get the absolute path of a file relative to the crate root
    fn get_crate_abs_path(relative_path: &str) -> PathBuf {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(manifest_dir).join(relative_path).canonicalize().unwrap()
    }

    // Helper to create the expected cursor:// link format
    fn make_link_url(abs_path: &Path, line: u32) -> String {
        format!("cursor://file/{}:{}", abs_path.to_string_lossy(), line)
    }

    // Helper to format the OSC 8 sequence
    fn make_osc8_link(url: &str, text: &str) -> String {
        format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
    }

    #[test]
    fn test_no_match() {
        let input = "This is a simple line of text without any paths.";
        let cwd = env::current_dir().unwrap();
        let expected = input;
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_simple_path_line() {
        let input = "Found error in Cargo.toml:5";
        let cwd = env::current_dir().unwrap();
        let abs_path = get_crate_abs_path("Cargo.toml");
        let url = make_link_url(&abs_path, 5);
        let link_text = "Cargo.toml:5";
        let expected = format!("Found error in {}", make_osc8_link(&url, link_text));
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_python_traceback() {
        // We use Cargo.toml here as a stand-in for a python file for existence check
        let abs_path = get_crate_abs_path("Cargo.toml");
        let abs_path_str = abs_path.to_string_lossy();
        // Input string with leading spaces
        let input = format!("  File \"{}\", line 10, in <module>", abs_path_str);
        let cwd = env::current_dir().unwrap();

        let url = make_link_url(&abs_path, 10);
        // The link text should now include the leading spaces because the regex matches them
        let link_text_owned = format!("  File \"{}\", line 10", abs_path_str);
        // Expected output has the hyperlink applied to the text including spaces, followed by the rest
        let expected = format!("{}, in <module>", make_osc8_link(&url, &link_text_owned));

        let actual = transform(&input, &cwd);
        assert_eq!(actual, expected);
    }

     #[test]
    fn test_ipdb_traceback() {
        // Test with absolute path
        let abs_path = get_crate_abs_path("Cargo.toml");
        let abs_path_str = abs_path.to_string_lossy();
        let input = format!("> {}(22) some_func()", abs_path_str);
        let cwd = env::current_dir().unwrap();

        let url = make_link_url(&abs_path, 22);
        let link_text_owned = format!("> {}(22)", abs_path_str);
        let expected = format!("{} some_func()", make_osc8_link(&url, &link_text_owned));

        let actual = transform(&input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_ansi_color() {
        let input = "Error: \x1b[31mCargo.toml:15\x1b[0m is bad.";
        let cwd = env::current_dir().unwrap();
        let abs_path = get_crate_abs_path("Cargo.toml");
        let url = make_link_url(&abs_path, 15);
        // The link text now INCLUDES the trailing ANSI reset code
        let link_text_with_ansi = "Cargo.toml:15\x1b[0m";
        // The link should be inserted *after* the opening color code,
        // wrap the text INCLUDING its trailing reset code.
        let expected = format!(
            "Error: \x1b[31m{} is bad.",
            make_osc8_link(&url, link_text_with_ansi)
        );
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_multiple_matches() {
        let input = "See Cargo.toml:3 and src/main.rs:10 for details.";
        let cwd = env::current_dir().unwrap();
        let path1 = get_crate_abs_path("Cargo.toml");
        let path2 = get_crate_abs_path("src/main.rs");
        let url1 = make_link_url(&path1, 3);
        let url2 = make_link_url(&path2, 10);
        let link_text1 = "Cargo.toml:3";
        let link_text2 = "src/main.rs:10";
        let expected = format!(
            "See {} and {} for details.",
            make_osc8_link(&url1, link_text1),
            make_osc8_link(&url2, link_text2)
        );
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

     #[test]
    fn test_relative_paths() {
        // Assumes src/main.rs exists
        let input = "Check src/main.rs:1 for setup.";
        let cwd = env::current_dir().unwrap(); // Should be crate root
        let abs_path = get_crate_abs_path("src/main.rs");
        let url = make_link_url(&abs_path, 1);
        let link_text = "src/main.rs:1";
        let expected = format!("Check {} for setup.", make_osc8_link(&url, link_text));
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_non_existent_path() {
        // This path:line should not be linked because the file doesn't exist
        // and it doesn't contain typical path separators like / or .
        let input = "NonExistentFile:123 is not a real file.";
        let cwd = env::current_dir().unwrap();
        let expected = input; // Expect no transformation
        let actual = transform(input, &cwd);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_python_traceback_with_internal_ansi() {
        // Test case demonstrating the limitation with ANSI codes inside a potential match.
        let cwd = env::current_dir().unwrap();
        let abs_path = get_crate_abs_path("Cargo.toml"); // Use existing file
        let abs_path_str = abs_path.to_string_lossy();
        let line_num = 5; // Arbitrary line number

        // 1. Define the exact text segment that *should* be hyperlinked (incl. internal ANSI)
        let text_to_be_hyperlinked = format!(
            "  File \x1b[35m\"{}\"\x1b[0m, line \x1b[35m{}\x1b[0m",
            abs_path_str, line_num
        );

        // 2. Construct the full input string using the segment above
        let input = format!(
            "Traceback (most recent call last):\n{}, in \x1b[35m<module>\x1b[0m\n    raise ValueError\n \x1b[1;35mValueError\x1b[0m",
            text_to_be_hyperlinked
        );

        // --- Actual ---
        let actual = transform(&input, &cwd);

        // --- Assert ---
        // This assertion should FAIL with the current transform logic because it cannot
        // match the pattern across the internal ANSI codes.
        // 3. Define the *expected* (linked) output (This is the target state)
        let link_url = make_link_url(&abs_path, line_num);
        let expected_linked_segment = make_osc8_link(&link_url, &text_to_be_hyperlinked);
        let expected = format!(
            "Traceback (most recent call last):\n{}, in \x1b[35m<module>\x1b[0m\n    raise ValueError\n \x1b[1;35mValueError\x1b[0m",
            expected_linked_segment
        );
        assert_eq!(
            actual,
            expected,
            "Test expects hyperlink around 'File..., line...' segment, including internal ANSI"
        );

        // --- To make this test PASS with CURRENT logic (demonstrating the bug): ---
        // Comment out the assertion above and uncomment the one below.
        // This asserts that the output is identical to the input (no link added).
        // assert_eq!(
        //     actual,
        //     input, // Assert no change occurred
        //     "Test currently expects NO link due to internal ANSI (modify test to see failure)"
        // );
    }
}
