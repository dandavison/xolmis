// TODO
// /Users/dan/src/temporalio/nexus-sdk-python/tests/handler/test_service_handler_decorator_collects_expected_operation_definitions.py(150)<module>()
// -> class SyncOperationWithCallableInstance(_TestCase):
//   /Users/dan/src/temporalio/nexus-sdk-python/tests/handler/test_service_handler_decorator_collects_expected_operation_definitions.py(158)SyncOperationWithCallableInstance()
// -> class Service:
//   /Users/dan/src/temporalio/nexus-sdk-python/tests/handler/test_service_handler_decorator_collects_expected_operation_definitions.py(167)Service()
// -> _sync_operation_with_callable_instance = sync_operation(
//   /Users/dan/src/temporalio/nexus-sdk-python/src/nexusrpc/handler/_decorators.py(301)sync_operation()
// -> return decorator(start)
//   /Users/dan/src/temporalio/nexus-sdk-python/src/nexusrpc/handler/_decorators.py(280)decorator()
// -> input_type, output_type = get_start_method_input_and_output_type_annotations(  # type: ignore[var-annotated]
// > /Users/dan/src/temporalio/nexus-sdk-python/src/nexusrpc/handler/_util.py(42)get_start_method_input_and_output_type_annotations()
// -> pdb.set_trace()

use lazy_static::lazy_static;
use regex::Regex;

// Structure holding the static definition data for a rule
pub struct RuleData {
    pub name: &'static str,
    pub regex_str: &'static str,
    pub path_group_name: &'static str,
    pub line_group_name: Option<&'static str>,
}

// Structure holding the compiled regex and other rule info
#[derive(Clone)]
pub struct CompiledRule {
    pub name: &'static str,
    pub regex: Regex, // Compiled regex
    pub path_group_index: usize,
    pub line_group_index: Option<usize>,
}

// Regex to capture file paths, optionally followed by :line_number.
// Matches paths starting with /, ./, ../, ~, or C:\, or containing at least one / or \.
// It avoids matching URLs like http://... by requiring path characters.
const FILE_PATH_REGEX_OPT_LINE: &str = r"(?P<path>(?:(?:~|\.|/|[a-zA-Z]:\\)[a-zA-Z0-9._\\/~-]+)|(?:\b[a-zA-Z0-9._~-]+[\\/][a-zA-Z0-9._\\/~-]+))(?::(?P<line>\d+))?\b";

// Python traceback pattern (optional line)
const PYTHON_TRACE_REGEX_OPT_LINE: &str = r#"^\s*File "(?P<path>.*?)"(?:, line (?P<line>\d+))?"#;

// IPDB traceback pattern (optional line)
const IPDB_TRACE_REGEX_OPT_LINE: &str = r"^>\s*(?P<path>[^(]+)(?:\((?P<line>\d+)\))?";

// Define the raw rule data as a const array
const RULES_DATA: &[RuleData] = &[
    RuleData {
        name: "FilePath",
        regex_str: FILE_PATH_REGEX_OPT_LINE,
        path_group_name: "path",
        line_group_name: Some("line"),
    },
    RuleData {
        name: "PythonTraceback",
        regex_str: PYTHON_TRACE_REGEX_OPT_LINE,
        path_group_name: "path",
        line_group_name: Some("line"),
    },
    RuleData {
        name: "IpdbTraceback",
        regex_str: IPDB_TRACE_REGEX_OPT_LINE,
        path_group_name: "path",
        line_group_name: Some("line"),
    },
];

lazy_static! {
    // This static variable holds the compiled rules.
    // It is initialized only once, the first time get_rules() is called.
    static ref COMPILED_RULES: Vec<CompiledRule> = {
        RULES_DATA.iter().map(|rule_data| {
            let re = Regex::new(rule_data.regex_str).expect("Failed to compile regex");

            // Find the capture group index for the path by name
            let path_group_index = re
                .capture_names()
                .position(|name| name == Some(rule_data.path_group_name))
                .unwrap_or_else(|| panic!("Path capture group '{}' not found in regex for rule '{}'", rule_data.path_group_name, rule_data.name));

            // Find the capture group index for the line number by name, if specified
            let line_group_index = rule_data.line_group_name.and_then(|name| {
                re.capture_names()
                    .position(|n| n == Some(name))
                    // Log a warning if the named group exists in RuleData but not in regex? For now, just return None.
                    // .or_else(|| { eprintln!("Warning: Optional line group '{}' not found in regex for rule '{}'", name, rule_data.name); None })
            });

            CompiledRule {
                name: rule_data.name,
                regex: re,
                path_group_index,
                line_group_index,
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
        let rules = get_compiled_rules();
        assert!(rules.iter().all(|r| !r.name.is_empty()));
        // Check one rule's indices specifically using capture names
        let file_rule = rules.iter().find(|r| r.name == "FilePath").unwrap();
        // Find index by name for verification
        let path_idx = file_rule
            .regex
            .capture_names()
            .position(|n| n == Some("path"));
        let line_idx = file_rule
            .regex
            .capture_names()
            .position(|n| n == Some("line"));
        assert_eq!(Some(file_rule.path_group_index), path_idx);
        assert_eq!(file_rule.line_group_index, line_idx);
    }

    #[test]
    fn test_file_path_regex() {
        let rule = get_compiled_rules()
            .iter()
            .find(|r| r.name == "FilePath")
            .unwrap();
        // Path with line
        let caps = rule.regex.captures("src/main.rs:10").unwrap();
        assert_eq!(caps.name("path").unwrap().as_str(), "src/main.rs");
        assert_eq!(caps.name("line").unwrap().as_str(), "10");

        // Path without line
        let caps = rule.regex.captures("./relative/path/file.txt").unwrap();
        assert_eq!(
            caps.name("path").unwrap().as_str(),
            "./relative/path/file.txt"
        );
        assert!(caps.name("line").is_none()); // Check optional group by name

        // Absolute path
        let caps = rule.regex.captures("/absolute/path/to/file").unwrap();
        assert_eq!(
            caps.name("path").unwrap().as_str(),
            "/absolute/path/to/file"
        );
        assert!(caps.name("line").is_none()); // Check optional group by name

        // Path with Windows drive letter
        let caps = rule.regex.captures("C:\\Users\\Test\\file.rs:123").unwrap();
        assert_eq!(
            caps.name("path").unwrap().as_str(),
            "C:\\Users\\Test\\file.rs"
        );
        assert_eq!(caps.name("line").unwrap().as_str(), "123");

        // Invalid path (no / or \) - ensure it doesn't match our stricter path regex
        assert!(rule.regex.captures("plainfile:10").is_none());
        // URL - should not match
        assert!(rule.regex.captures("http://example.com:80").is_none());
        // Path ending in :
        let caps = rule.regex.captures("/path/ends/with:").unwrap(); // Regex allows path ending like this
        assert_eq!(caps.name("path").unwrap().as_str(), "/path/ends/with");
        assert!(caps.name("line").is_none()); // Line group should be None
    }

    #[test]
    fn test_python_trace_regex() {
        let rule = get_compiled_rules()
            .iter()
            .find(|r| r.name == "PythonTraceback")
            .unwrap();
        // With line
        let caps = rule
            .regex
            .captures("  File \"/path/to/my_module.py\", line 123")
            .unwrap();
        assert_eq!(caps.name("path").unwrap().as_str(), "/path/to/my_module.py");
        assert_eq!(caps.name("line").unwrap().as_str(), "123");

        // Without line
        let caps = rule.regex.captures("  File \"/another/path.py\"").unwrap();
        assert_eq!(caps.name("path").unwrap().as_str(), "/another/path.py");
        assert!(caps.name("line").is_none()); // Check optional group by name
    }

    #[test]
    fn test_ipdb_trace_regex() {
        let rule = get_compiled_rules()
            .iter()
            .find(|r| r.name == "IpdbTraceback")
            .unwrap();
        // With line
        let caps = rule
            .regex
            .captures("> /path/to/debugger.py(45)some_func()")
            .unwrap();
        assert_eq!(caps.name("path").unwrap().as_str(), "/path/to/debugger.py");
        assert_eq!(caps.name("line").unwrap().as_str(), "45");

        // Without line
        let caps = rule.regex.captures("> /another/script.py").unwrap();
        assert_eq!(caps.name("path").unwrap().as_str(), "/another/script.py");
        assert!(caps.name("line").is_none()); // Check optional group by name
    }
}
