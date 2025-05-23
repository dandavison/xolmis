#![allow(dead_code)]

pub mod iterator;

mod console_tests;

use std::borrow::Cow;

use ansi_term::Style;
use itertools::Itertools;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use iterator::{AnsiElementIterator, Element};

pub const ANSI_CSI_CLEAR_TO_EOL: &str = "\x1b[0K";
pub const ANSI_CSI_CLEAR_TO_BOL: &str = "\x1b[1K";
pub const ANSI_SGR_BOLD: &str = "\x1b[1m";
pub const ANSI_SGR_RESET: &str = "\x1b[0m";
pub const ANSI_SGR_REVERSE: &str = "\x1b[7m";
pub const ANSI_SGR_UNDERLINE: &str = "\x1b[4m";

pub fn strip_ansi_codes(s: &str) -> String {
    strip_ansi_codes_from_strings_iterator(ansi_strings_iterator(s))
}

pub fn measure_text_width(s: &str) -> usize {
    ansi_strings_iterator(s).fold(0, |acc, (element, is_ansi)| {
        acc + if is_ansi { 0 } else { element.width() }
    })
}

fn truncate_str_impl<'a>(
    s: &'a str,
    display_width: usize,
    tail: &str,
    fill2w: Option<char>,
) -> Cow<'a, str> {
    let items = ansi_strings_iterator(s).collect::<Vec<(&str, bool)>>();
    let width = strip_ansi_codes_from_strings_iterator(items.iter().copied()).width();
    if width <= display_width {
        return Cow::from(s);
    }
    let result_tail = if !tail.is_empty() {
        truncate_str_impl(tail, display_width, "", fill2w).to_string()
    } else {
        String::new()
    };
    let mut used = measure_text_width(&result_tail);
    let mut result = String::new();
    for (t, is_ansi) in items {
        if !is_ansi {
            for g in t.graphemes(true) {
                let width_of_grapheme = g.width();
                if used + width_of_grapheme > display_width {
                    // Handle case "2." mentioned in `truncate_str` docs and fill the
                    // hole left by double-width (2w) truncation.
                    if let Some(fillchar) = fill2w {
                        if width_of_grapheme == 2 && used < display_width {
                            result.push(fillchar);
                        } else if width_of_grapheme > 2 {
                            // Should not happen, this means either unicode_segmentation
                            // graphemes are too wide, or the unicode_width is calculated wrong.
                            // Fallback:
                            debug_assert!(width_of_grapheme <= 2, "strange grapheme width");
                            for _ in 0..display_width.saturating_sub(used) {
                                result.push(fillchar);
                            }
                        }
                    }
                    break;
                }
                result.push_str(g);
                used += width_of_grapheme;
            }
        } else {
            result.push_str(t);
        }
    }

    result.push_str(&result_tail);
    Cow::from(result)
}

/// Truncate string such that `tail` is present as a suffix, preceded by as much of `s` as can be
/// displayed in the requested width. Even with `tail` empty the result may not be a prefix of `s`.
// Return string constructed as follows:
// 1. `display_width` characters are available. If the string fits, return it.
//
// 2. If a double-width (fullwidth) grapheme has to be cut in the following steps, replace the first
//    half with a space (' '). If this happens the result is no longer a prefix of the input.
//
// 3. Contribute graphemes and ANSI escape sequences from `tail` until either (1) `tail` is
//    exhausted, or (2) the display width of the result would exceed `display_width`.
//
// 4. If tail was exhausted, then contribute graphemes and ANSI escape sequences from `s` until the
//    display_width of the result would exceed `display_width`.
pub fn truncate_str<'a>(s: &'a str, display_width: usize, tail: &str) -> Cow<'a, str> {
    truncate_str_impl(s, display_width, tail, Some(' '))
}

/// Truncate string `s` so it fits into `display_width`, ignoring any ANSI escape sequences when
/// calculating the width. If a double-width ("fullwidth") grapheme has to be cut, it is omitted and
/// the resulting string is *shorter* than `display_width`. But this way the result is always a
/// prefix of the input `s`.
pub fn truncate_str_short(s: &str, display_width: usize) -> Cow<str> {
    truncate_str_impl(s, display_width, "", None)
}

pub fn parse_style_sections(s: &str) -> Vec<(ansi_term::Style, &str)> {
    let mut sections = Vec::new();
    let mut curr_style = Style::default();
    for element in AnsiElementIterator::new(s) {
        match element {
            Element::Text(start, end) => sections.push((curr_style, &s[start..end])),
            Element::Sgr(style, _, _) => curr_style = style,
            _ => {}
        }
    }
    sections
}

// Return the first CSI element, if any, as an `ansi_term::Style`.
pub fn parse_first_style(s: &str) -> Option<ansi_term::Style> {
    AnsiElementIterator::new(s).find_map(|el| match el {
        Element::Sgr(style, _, _) => Some(style),
        _ => None,
    })
}

pub fn string_starts_with_ansi_style_sequence(s: &str) -> bool {
    AnsiElementIterator::new(s)
        .next()
        .map(|el| matches!(el, Element::Sgr(_, _, _)))
        .unwrap_or(false)
}

/// Return string formed from a byte slice starting at byte position `start`, where the index
/// counts bytes in non-ANSI-escape-sequence content only. All ANSI escape sequences in the
/// original string are preserved.
pub fn ansi_preserving_slice(s: &str, start: usize) -> String {
    AnsiElementIterator::new(s)
        .scan(0, |index, element| {
            // `index` is the index in non-ANSI-escape-sequence content.
            Some(match element {
                Element::Sgr(_, a, b) => &s[a..b],
                Element::Csi(a, b) => &s[a..b],
                Element::Esc(a, b) => &s[a..b],
                Element::Osc(a, b) => &s[a..b],
                Element::Text(a, b) => {
                    let i = *index;
                    *index += b - a;
                    if *index <= start {
                        // This text segment ends before start, so contributes no bytes.
                        ""
                    } else if i > start {
                        // This section starts after `start`, so contributes all its bytes.
                        &s[a..b]
                    } else {
                        // This section contributes those bytes that are >= start
                        &s[(a + start - i)..b]
                    }
                }
            })
        })
        .join("")
}

/// Return the byte index in `s` of the i-th text byte in `s`. I.e. `i` counts
/// bytes in non-ANSI-escape-sequence content only.
pub fn ansi_preserving_index(s: &str, i: usize) -> Option<usize> {
    let mut index = 0;
    for element in AnsiElementIterator::new(s) {
        if let Element::Text(a, b) = element {
            index += b - a;
            if index > i {
                return Some(b - (index - i));
            }
        }
    }
    None
}

fn ansi_strings_iterator(s: &str) -> impl Iterator<Item = (&str, bool)> {
    AnsiElementIterator::new(s).map(move |el| match el {
        Element::Sgr(_, i, j) => (&s[i..j], true),
        Element::Csi(i, j) => (&s[i..j], true),
        Element::Esc(i, j) => (&s[i..j], true),
        Element::Osc(i, j) => (&s[i..j], true),
        Element::Text(i, j) => (&s[i..j], false),
    })
}

fn strip_ansi_codes_from_strings_iterator<'a>(
    strings: impl Iterator<Item = (&'a str, bool)>,
) -> String {
    strings
        .filter_map(|(el, is_ansi)| if !is_ansi { Some(el) } else { None })
        .join("")
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    // Note that src/ansi/console_tests.rs contains additional test coverage for this module.
    use super::{
        ansi_preserving_index, ansi_preserving_slice, measure_text_width, parse_first_style,
        string_starts_with_ansi_style_sequence, strip_ansi_codes, truncate_str, truncate_str_short,
    };

    #[test]
    fn test_strip_ansi_codes() {
        for s in &["src/ansi/mod.rs", "バー", "src/ansi/modバー.rs"] {
            assert_eq!(strip_ansi_codes(s), *s);
        }
        assert_eq!(strip_ansi_codes("\x1b[31mバー\x1b[0m"), "バー");
    }

    #[test]
    fn test_measure_text_width() {
        assert_eq!(measure_text_width("src/ansi/mod.rs"), 15);
        assert_eq!(measure_text_width("バー"), 4);
        assert_eq!(measure_text_width("src/ansi/modバー.rs"), 19);
        assert_eq!(measure_text_width("\x1b[31mバー\x1b[0m"), 4);
        assert_eq!(measure_text_width("a\nb\n"), 2);
    }

    #[test]
    fn test_strip_ansi_codes_osc_hyperlink() {
        assert_eq!(strip_ansi_codes("\x1b[38;5;4m\x1b]8;;file:///Users/dan/src/delta/src/ansi/mod.rs\x1b\\src/ansi/mod.rs\x1b]8;;\x1b\\\x1b[0m\n"),
                   "src/ansi/mod.rs\n");
    }

    #[test]
    fn test_measure_text_width_osc_hyperlink() {
        assert_eq!(measure_text_width("\x1b[38;5;4m\x1b]8;;file:///Users/dan/src/delta/src/ansi/mod.rs\x1b\\src/ansi/mod.rs\x1b]8;;\x1b\\\x1b[0m"),
                   measure_text_width("src/ansi/mod.rs"));
    }

    #[test]
    fn test_measure_text_width_osc_hyperlink_non_ascii() {
        assert_eq!(measure_text_width("\x1b[38;5;4m\x1b]8;;file:///Users/dan/src/delta/src/ansi/mod.rs\x1b\\src/ansi/modバー.rs\x1b]8;;\x1b\\\x1b[0m"),
                   measure_text_width("src/ansi/modバー.rs"));
    }

    #[test]
    fn test_parse_first_style() {
        let minus_line_from_unconfigured_git = "\x1b[31m-____\x1b[m\n";
        let style = parse_first_style(minus_line_from_unconfigured_git);
        let expected_style = ansi_term::Style {
            foreground: Some(ansi_term::Color::Red),
            ..ansi_term::Style::default()
        };
        assert_eq!(Some(expected_style), style);
    }

    #[test]
    fn test_string_starts_with_ansi_escape_sequence() {
        assert!(!string_starts_with_ansi_style_sequence(""));
        assert!(!string_starts_with_ansi_style_sequence("-"));
        assert!(string_starts_with_ansi_style_sequence(
            "\x1b[31m-XXX\x1b[m\n"
        ));
        assert!(string_starts_with_ansi_style_sequence("\x1b[32m+XXX"));
    }

    #[test]
    fn test_ansi_preserving_slice_and_index() {
        assert_eq!(ansi_preserving_slice("", 0), "");
        assert_eq!(ansi_preserving_index("", 0), None);

        assert_eq!(ansi_preserving_slice("0", 0), "0");
        assert_eq!(ansi_preserving_index("0", 0), Some(0));

        assert_eq!(ansi_preserving_slice("0", 1), "");
        assert_eq!(ansi_preserving_index("0", 1), None);

        let raw_string = "\x1b[1;35m0123456789\x1b[0m";
        assert_eq!(
            ansi_preserving_slice(raw_string, 1),
            "\x1b[1;35m123456789\x1b[0m"
        );
        assert_eq!(ansi_preserving_slice(raw_string, 7), "\x1b[1;35m789\x1b[0m");
        assert_eq!(ansi_preserving_index(raw_string, 0), Some(7));
        assert_eq!(ansi_preserving_index(raw_string, 1), Some(8));
        assert_eq!(ansi_preserving_index(raw_string, 7), Some(14));

        let raw_string = "\x1b[1;36m0\x1b[m\x1b[1;36m123456789\x1b[m\n";
        assert_eq!(
            ansi_preserving_slice(raw_string, 1),
            "\x1b[1;36m\x1b[m\x1b[1;36m123456789\x1b[m\n"
        );
        assert_eq!(ansi_preserving_index(raw_string, 0), Some(7));
        assert_eq!(ansi_preserving_index(raw_string, 1), Some(18));
        assert_eq!(ansi_preserving_index(raw_string, 7), Some(24));

        let raw_string = "\x1b[1;36m012345\x1b[m\x1b[1;36m6789\x1b[m\n";
        assert_eq!(
            ansi_preserving_slice(raw_string, 3),
            "\x1b[1;36m345\x1b[m\x1b[1;36m6789\x1b[m\n"
        );
        assert_eq!(ansi_preserving_index(raw_string, 0), Some(7));
        assert_eq!(ansi_preserving_index(raw_string, 1), Some(8));
        assert_eq!(ansi_preserving_index(raw_string, 7), Some(24));
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("1", 1, ""), "1");
        assert_eq!(truncate_str("12", 1, ""), "1");
        assert_eq!(truncate_str("123", 2, "s"), "1s");
        assert_eq!(truncate_str("123", 2, "→"), "1→");
        assert_eq!(truncate_str("12ݶ", 1, "ݶ"), "ݶ");
    }

    #[test]
    fn test_truncate_str_at_double_width_grapheme() {
        let one_double_four = "1＃4";
        let double = "／";
        assert_eq!(one_double_four.width(), 4);
        assert_eq!(double.width(), 2);

        assert_eq!(truncate_str(one_double_four, 1, ""), "1");
        assert_eq!(truncate_str(one_double_four, 2, ""), "1 ");
        assert_eq!(truncate_str(one_double_four, 3, ""), "1＃");
        assert_eq!(truncate_str(one_double_four, 4, ""), "1＃4");

        assert_eq!(truncate_str_short(one_double_four, 1), "1");
        assert_eq!(truncate_str_short(one_double_four, 2), "1"); // !!
        assert_eq!(truncate_str_short(one_double_four, 3), "1＃");
        assert_eq!(truncate_str_short(one_double_four, 4), "1＃4");

        assert_eq!(truncate_str(one_double_four, 1, double), " ");
        assert_eq!(truncate_str(one_double_four, 2, double), "／");
        assert_eq!(truncate_str(one_double_four, 3, double), "1／");
        assert_eq!(truncate_str(one_double_four, 4, double), "1＃4");

        assert_eq!(truncate_str(one_double_four, 0, ""), "");
        assert_eq!(truncate_str(one_double_four, 0, double), "");
        assert_eq!(truncate_str_short(one_double_four, 0), "");

        assert_eq!(truncate_str(double, 0, double), "");
        assert_eq!(truncate_str(double, 1, double), " ");
        assert_eq!(truncate_str(double, 2, double), double);

        assert_eq!(truncate_str_short(double, 0), "");
        assert_eq!(truncate_str_short(double, 1), "");
        assert_eq!(truncate_str_short(double, 2), double);
    }
}
