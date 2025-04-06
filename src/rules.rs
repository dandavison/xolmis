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
        regex_str: r"^[->]\s*(\S+)\((\d+)\)",
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