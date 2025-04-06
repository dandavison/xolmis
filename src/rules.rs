use regex::Regex;
use lazy_static::lazy_static;

// Structure holding the static definition data for a rule
pub struct RuleData {
    pub name: &'static str,
    pub regex_str: &'static str,
    pub path_group_index: usize,
    pub line_group_index: usize,
}

// Structure holding the compiled regex and other rule info
#[derive(Clone)]
pub struct CompiledRule {
    pub name: &'static str,
    pub regex: Regex, // Compiled regex
    pub path_group_index: usize,
    pub line_group_index: usize,
}

// Define the raw rule data as a const array
const RULES_DATA: &[RuleData] = &[
    RuleData {
        name: "PythonTrace",
        regex_str: r#"File "([^"]+)", line (\d+)"#,
        path_group_index: 1,
        line_group_index: 2,
    },
    RuleData {
        name: "IpdbTrace",
        regex_str: r"^(?:->|>)\s+(\S+)\((\d+)\)",
        path_group_index: 1,
        line_group_index: 2,
    },
    RuleData {
        name: "FilePathLine",
        regex_str: r"([a-zA-Z0-9-_./]+):(\d+)",
        path_group_index: 1,
        line_group_index: 2,
    },
];

lazy_static! {
    // This static variable holds the compiled rules.
    // It is initialized only once, the first time get_rules() is called.
    static ref COMPILED_RULES: Vec<CompiledRule> = {
        RULES_DATA.iter().map(|rule_data| {
            CompiledRule {
                name: rule_data.name,
                // Compile the regex string here
                regex: Regex::new(rule_data.regex_str).expect("Failed to compile regex"),
                path_group_index: rule_data.path_group_index,
                line_group_index: rule_data.line_group_index,
            }
        }).collect()
    };
}

// Returns a slice of the compiled rules.
pub fn get_compiled_rules() -> &'static [CompiledRule] {
    &COMPILED_RULES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_compiled_rules_count() {
        // Ensure the number of compiled rules matches the static definitions
        assert_eq!(get_compiled_rules().len(), RULES_DATA.len());
    }

    #[test]
    fn test_rule_compilation_and_content() {
        let compiled_rules = get_compiled_rules();
        // Iterate and check if names and indices match the source data
        for (i, compiled) in compiled_rules.iter().enumerate() {
            assert_eq!(compiled.name, RULES_DATA[i].name);
            assert_eq!(compiled.path_group_index, RULES_DATA[i].path_group_index);
            assert_eq!(compiled.line_group_index, RULES_DATA[i].line_group_index);
            // Basic check that the regex string looks similar after compilation
            // Note: Regex::as_str might differ slightly from the original but should be functionally equivalent.
            // This is just a sanity check. A full regex equivalence check is complex.
            // assert!(compiled.regex.as_str().contains(RULES_DATA[i].regex_str)); // This can be too strict/brittle
        }
    }

    fn find_rule(name: &str) -> Option<&'static CompiledRule> {
        get_compiled_rules().iter().find(|r| r.name == name)
    }

    #[test]
    fn test_python_trace_regex() {
        let rule = find_rule("PythonTrace").expect("PythonTrace rule not found");
        let text = r#"  File "/path/to/your_file.py", line 123, in some_function"#;
        let caps = rule.regex.captures(text).expect("Regex should capture");

        assert_eq!(caps.get(rule.path_group_index).unwrap().as_str(), "/path/to/your_file.py");
        assert_eq!(caps.get(rule.line_group_index).unwrap().as_str(), "123");

        let no_match_text = "Just some regular text";
        assert!(rule.regex.captures(no_match_text).is_none());
    }

    #[test]
    fn test_ipdb_trace_regex() {
        // Find the raw rule data first
        let rule_data = RULES_DATA.iter().find(|r| r.name == "IpdbTrace").expect("IpdbTrace rule data not found");
        // Compile the regex specifically for this test to ensure we use the current definition
        let rule_regex = Regex::new(rule_data.regex_str).expect("Failed to compile IpdbTrace regex for test");

        let text = r"-> /path/to/another_file.py(45)function_name()";
        let caps = rule_regex.captures(text).expect("Regex should capture");

        assert_eq!(caps.get(rule_data.path_group_index).unwrap().as_str(), "/path/to/another_file.py");
        assert_eq!(caps.get(rule_data.line_group_index).unwrap().as_str(), "45");

        let text_arrow_no_space = r">/path/fail.py(1)f()"; // Should not match if space is required after ->
        assert!(rule_regex.captures(text_arrow_no_space).is_none()); // Based on current regex `^[->]\s+`

        let text_with_space = r"->  spaced/path.py(99) func()";
         let caps_space = rule_regex.captures(text_with_space).expect("Regex should capture with space");
        assert_eq!(caps_space.get(rule_data.path_group_index).unwrap().as_str(), "spaced/path.py");
        assert_eq!(caps_space.get(rule_data.line_group_index).unwrap().as_str(), "99");
    }

     #[test]
    fn test_file_path_line_regex() {
        let rule = find_rule("FilePathLine").expect("FilePathLine rule not found");

        let text1 = "src/main.rs:50";
        let caps1 = rule.regex.captures(text1).expect("Regex should capture path:line");
        assert_eq!(caps1.get(rule.path_group_index).unwrap().as_str(), "src/main.rs");
        assert_eq!(caps1.get(rule.line_group_index).unwrap().as_str(), "50");

        let text2 = "./relative/path.txt:1";
        let caps2 = rule.regex.captures(text2).expect("Regex should capture relative path:line");
        assert_eq!(caps2.get(rule.path_group_index).unwrap().as_str(), "./relative/path.txt");
        assert_eq!(caps2.get(rule.line_group_index).unwrap().as_str(), "1");

        let text3 = "nodirfile:12";
        let caps3 = rule.regex.captures(text3).expect("Regex should capture file:line");
        assert_eq!(caps3.get(rule.path_group_index).unwrap().as_str(), "nodirfile");
        assert_eq!(caps3.get(rule.line_group_index).unwrap().as_str(), "12");

        let text4 = "path_with_underscores_and-hyphens.ext:999";
        let caps4 = rule.regex.captures(text4).expect("Regex should capture complex filename");
        assert_eq!(caps4.get(rule.path_group_index).unwrap().as_str(), "path_with_underscores_and-hyphens.ext");
        assert_eq!(caps4.get(rule.line_group_index).unwrap().as_str(), "999");

        let text_no_line = "just/a/path";
        assert!(rule.regex.captures(text_no_line).is_none());

        let text_no_path = ":123";
        assert!(rule.regex.captures(text_no_path).is_none()); // The current regex requires a path part
    }
} 