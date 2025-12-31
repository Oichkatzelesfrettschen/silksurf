//! test262 test metadata parser
//!
//! Parses YAML frontmatter from test262 test files.
//! Format: https://github.com/tc39/test262/blob/main/CONTRIBUTING.md

use std::collections::HashSet;

/// Parsed test262 metadata from YAML frontmatter
#[derive(Debug, Clone, Default)]
pub struct TestMetadata {
    /// Test description
    pub description: String,
    /// ES spec section info
    pub esid: Option<String>,
    /// ES spec section (legacy)
    pub es5id: Option<String>,
    /// ES6 section (legacy)
    pub es6id: Option<String>,
    /// Negative test expectation
    pub negative: Option<NegativeExpectation>,
    /// Required includes (harness files)
    pub includes: Vec<String>,
    /// Test flags
    pub flags: TestFlags,
    /// Required features
    pub features: HashSet<String>,
    /// Locale requirements
    pub locale: Vec<String>,
    /// Additional info
    pub info: Option<String>,
    /// Author
    pub author: Option<String>,
}

/// Expected negative outcome
#[derive(Debug, Clone)]
pub struct NegativeExpectation {
    /// When the error should occur
    pub phase: NegativePhase,
    /// Expected error type
    pub error_type: String,
}

/// Phase when negative test should fail
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegativePhase {
    /// Parsing phase
    Parse,
    /// Early error (before execution)
    Early,
    /// Resolution phase (modules)
    Resolution,
    /// Runtime execution
    Runtime,
}

/// Test execution flags
#[derive(Debug, Clone, Default)]
pub struct TestFlags {
    /// Only run in strict mode
    pub only_strict: bool,
    /// Only run in non-strict (sloppy) mode
    pub no_strict: bool,
    /// Test is a module
    pub module: bool,
    /// Test is raw (no harness)
    pub raw: bool,
    /// Test is async
    pub is_async: bool,
    /// Test generates output
    pub generated: bool,
    /// Can be run in Atomics agent
    pub can_block_is_false: bool,
}

impl TestMetadata {
    /// Parse metadata from test file content
    pub fn parse(content: &str) -> Option<Self> {
        // Find YAML frontmatter between /*--- and ---*/
        let start = content.find("/*---")?;
        let end = content[start..].find("---*/")?;
        let yaml = &content[start + 5..start + end].trim();

        let mut metadata = TestMetadata::default();

        for line in yaml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = parse_yaml_line(line) {
                match key {
                    "description" => metadata.description = value.to_string(),
                    "esid" => metadata.esid = Some(value.to_string()),
                    "es5id" => metadata.es5id = Some(value.to_string()),
                    "es6id" => metadata.es6id = Some(value.to_string()),
                    "info" => metadata.info = Some(value.to_string()),
                    "author" => metadata.author = Some(value.to_string()),
                    "flags" => metadata.flags = parse_flags(value),
                    "features" => metadata.features = parse_list(value),
                    "includes" => metadata.includes = parse_list(value).into_iter().collect(),
                    "locale" => metadata.locale = parse_list(value).into_iter().collect(),
                    "negative" => {
                        // Negative is a nested structure, parse specially
                        if let Some(neg) = parse_negative(yaml) {
                            metadata.negative = Some(neg);
                        }
                    }
                    _ => {} // Ignore unknown keys
                }
            }
        }

        Some(metadata)
    }

    /// Check if test requires a specific feature
    pub fn requires_feature(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }

    /// Check if test should run in strict mode
    pub fn should_run_strict(&self) -> bool {
        !self.flags.no_strict && !self.flags.raw
    }

    /// Check if test should run in non-strict mode
    pub fn should_run_non_strict(&self) -> bool {
        !self.flags.only_strict && !self.flags.raw && !self.flags.module
    }
}

fn parse_yaml_line(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let value = line[colon + 1..].trim();
    // Remove quotes if present
    let value = value.trim_matches('"').trim_matches('\'');
    Some((key, value))
}

fn parse_flags(value: &str) -> TestFlags {
    let mut flags = TestFlags::default();

    // Parse as YAML list: [onlyStrict, async]
    let items: Vec<&str> = if value.starts_with('[') {
        value.trim_matches(&['[', ']'][..])
            .split(',')
            .map(|s| s.trim())
            .collect()
    } else {
        vec![value]
    };

    for item in items {
        match item {
            "onlyStrict" => flags.only_strict = true,
            "noStrict" => flags.no_strict = true,
            "module" => flags.module = true,
            "raw" => flags.raw = true,
            "async" => flags.is_async = true,
            "generated" => flags.generated = true,
            "CanBlockIsFalse" => flags.can_block_is_false = true,
            _ => {}
        }
    }

    flags
}

fn parse_list(value: &str) -> HashSet<String> {
    if value.starts_with('[') {
        value.trim_matches(&['[', ']'][..])
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        let mut set = HashSet::new();
        if !value.is_empty() {
            set.insert(value.to_string());
        }
        set
    }
}

fn parse_negative(yaml: &str) -> Option<NegativeExpectation> {
    let mut phase = None;
    let mut error_type = None;

    // Look for negative block
    let neg_start = yaml.find("negative:")?;
    let neg_section = &yaml[neg_start..];

    for line in neg_section.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || (!line.starts_with("phase:") && !line.starts_with("type:") && !line.starts_with('-')) {
            // Check if we hit another top-level key
            if !line.starts_with(' ') && !line.starts_with('-') && line.contains(':') {
                break;
            }
            continue;
        }

        if let Some((key, value)) = parse_yaml_line(line.trim_start_matches('-').trim()) {
            match key {
                "phase" => {
                    phase = Some(match value {
                        "parse" => NegativePhase::Parse,
                        "early" => NegativePhase::Early,
                        "resolution" => NegativePhase::Resolution,
                        "runtime" => NegativePhase::Runtime,
                        _ => NegativePhase::Runtime,
                    });
                }
                "type" => error_type = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Some(NegativeExpectation {
        phase: phase.unwrap_or(NegativePhase::Runtime),
        error_type: error_type?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_metadata() {
        let content = r#"
/*---
description: Simple test
esid: sec-example
flags: [onlyStrict]
features: [BigInt, Symbol]
---*/
print("hello");
"#;

        let meta = TestMetadata::parse(content).unwrap();
        assert_eq!(meta.description, "Simple test");
        assert_eq!(meta.esid, Some("sec-example".to_string()));
        assert!(meta.flags.only_strict);
        assert!(meta.features.contains("BigInt"));
        assert!(meta.features.contains("Symbol"));
    }

    #[test]
    fn test_parse_negative() {
        let content = r#"
/*---
description: SyntaxError test
negative:
  phase: parse
  type: SyntaxError
---*/
function 123() {}
"#;

        let meta = TestMetadata::parse(content).unwrap();
        let neg = meta.negative.unwrap();
        assert_eq!(neg.phase, NegativePhase::Parse);
        assert_eq!(neg.error_type, "SyntaxError");
    }

    #[test]
    fn test_parse_includes() {
        let content = r#"
/*---
description: Test with includes
includes: [assert.js, compareArray.js]
---*/
assert(true);
"#;

        let meta = TestMetadata::parse(content).unwrap();
        assert!(meta.includes.contains(&"assert.js".to_string()));
        assert!(meta.includes.contains(&"compareArray.js".to_string()));
    }
}
