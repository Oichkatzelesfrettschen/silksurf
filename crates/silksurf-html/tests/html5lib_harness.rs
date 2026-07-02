use std::env;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};
use silksurf_html::{Attribute, Token, Tokenizer};

const HTML5LIB_TEST1_PASS_DESCRIPTIONS: &[&str] = &[
    "Single Start Tag",
    "Start Tag w/attribute",
    "Start Tag w/attribute no quotes",
    "Start/End Tag",
    "Two unclosed start tags",
    "Multiple atts",
    "Simple comment",
    "Comment, Central dash no space",
    "Comment, two central dashes",
];

#[test]
fn html5lib_tokenizer_smoke() {
    let base = env::var("HTML5LIB_TESTS_DIR")
        .unwrap_or_else(|_| "silksurf-extras/html5lib-tests/tokenizer".to_string());
    let test_path = Path::new(&base).join("test1.test");
    if !test_path.exists() {
        eprintln!("html5lib tests not found at {}", test_path.display());
        return;
    }

    let data = fs::read_to_string(&test_path).expect("read html5lib test file");
    let value: Value = serde_json::from_str(&data).expect("parse html5lib JSON");
    let tests = value
        .get("tests")
        .and_then(Value::as_array)
        .expect("html5lib tokenizer tests array");

    let mut executed = 0;
    for case in tests {
        let description = case
            .get("description")
            .and_then(Value::as_str)
            .expect("html5lib case description");
        if !HTML5LIB_TEST1_PASS_DESCRIPTIONS.contains(&description) {
            continue;
        }

        let input = case
            .get("input")
            .and_then(Value::as_str)
            .expect("html5lib case input");
        let expected = case
            .get("output")
            .and_then(Value::as_array)
            .expect("html5lib case output");
        let actual = tokenize_as_html5lib_values(input)
            .unwrap_or_else(|error| panic!("{description}: tokenizer error: {}", error.message));

        assert_eq!(&actual, expected, "{description}");
        executed += 1;
    }

    assert_eq!(executed, HTML5LIB_TEST1_PASS_DESCRIPTIONS.len());
}

fn tokenize_as_html5lib_values(input: &str) -> Result<Vec<Value>, silksurf_html::TokenizeError> {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);
    Ok(tokens
        .into_iter()
        .filter_map(token_as_html5lib_value)
        .collect())
}

fn token_as_html5lib_value(token: Token) -> Option<Value> {
    match token {
        Token::Doctype {
            name,
            public_id,
            system_id,
            force_quirks,
        } => Some(json!([
            "DOCTYPE",
            string_or_null(name),
            string_or_null(public_id),
            string_or_null(system_id),
            force_quirks
        ])),
        Token::StartTag {
            name,
            attributes,
            self_closing,
        } => Some(start_tag_as_html5lib_value(name, attributes, self_closing)),
        Token::EndTag { name } => Some(json!(["EndTag", name])),
        Token::Comment { data } => Some(json!(["Comment", data])),
        Token::Character { data } => Some(json!(["Character", data])),
        Token::Eof => None,
    }
}

fn start_tag_as_html5lib_value(
    name: String,
    attributes: Vec<Attribute>,
    self_closing: bool,
) -> Value {
    let mut attrs = Map::new();
    for attribute in attributes {
        attrs.insert(
            attribute.name,
            Value::String(attribute.value.unwrap_or_default()),
        );
    }

    let mut token = vec![
        Value::String("StartTag".to_string()),
        Value::String(name),
        Value::Object(attrs),
    ];
    if self_closing {
        token.push(Value::Bool(true));
    }
    Value::Array(token)
}

fn string_or_null(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}
