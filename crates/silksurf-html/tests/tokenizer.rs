use silksurf_html::{Attribute, Token, Tokenizer};

#[test]
fn tokenizes_basic_tags_and_text() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<html><body>hi</body></html>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "html".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::StartTag {
            name: "body".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::Character { data: "hi".into() },
        Token::EndTag { name: "body".into() },
        Token::EndTag { name: "html".into() },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_attributes_and_self_closing() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<img src=\"a.png\" alt=test/>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "img".into(),
            attributes: vec![
                Attribute {
                    name: "src".into(),
                    value: Some("a.png".into()),
                },
                Attribute {
                    name: "alt".into(),
                    value: Some("test".into()),
                },
            ],
            self_closing: true,
        },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_doctype_and_comment() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<!doctype html><!-- ok -->").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::Doctype {
            name: Some("html".into()),
            public_id: None,
            system_id: None,
            force_quirks: false,
        },
        Token::Comment { data: " ok ".into() },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_doctype_public_system_ids() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<!DOCTYPE html PUBLIC \"pub\" 'sys'>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::Doctype {
            name: Some("html".into()),
            public_id: Some("pub".into()),
            system_id: Some("sys".into()),
            force_quirks: false,
        },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn decodes_character_references() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<p title=\"Fish &amp; Chips\">Tom &amp; Jerry &#x3C;</p>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "p".into(),
            attributes: vec![Attribute {
                name: "title".into(),
                value: Some("Fish & Chips".into()),
            }],
            self_closing: false,
        },
        Token::Character {
            data: "Tom & Jerry <".into(),
        },
        Token::EndTag { name: "p".into() },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_script_raw_text() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<script>if (a < b && c > d) {}</script>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "script".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::Character {
            data: "if (a < b && c > d) {}".into(),
        },
        Token::EndTag {
            name: "script".into(),
        },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_style_raw_text() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<style>body{color:red}</style>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "style".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::Character {
            data: "body{color:red}".into(),
        },
        Token::EndTag {
            name: "style".into(),
        },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn decodes_numeric_character_reference_decimal() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<p>&#60;tag&#62;</p>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        Token::StartTag {
            name: "p".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::Character { data: "<tag>".into() },
        Token::EndTag { name: "p".into() },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}
