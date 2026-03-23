use silksurf_css::{CssToken, CssTokenizer};

#[test]
fn tokenizes_simple_rule() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("body { color: red; }").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Ident("body".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Ident("color".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::Ident("red".into()),
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_hash_and_comment() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("#id { /* note */ padding: 10px }").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Hash("id".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Whitespace,
        CssToken::Ident("padding".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::Dimension {
            value: "10".into(),
            unit: "px".into(),
        },
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_at_keyword_and_function() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer
        .feed("@media screen { color: rgb(10%); }")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::AtKeyword("media".into()),
        CssToken::Whitespace,
        CssToken::Ident("screen".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Ident("color".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::Function("rgb".into()),
        CssToken::Percentage("10".into()),
        CssToken::ParenClose,
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_cdo_cdc() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("<!-- -->").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Cdo,
        CssToken::Whitespace,
        CssToken::Cdc,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_url() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer
        .feed(".hero { background: url(\"img.png\"); }")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Delim('.'),
        CssToken::Ident("hero".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Ident("background".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::Url("img.png".into()),
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_escape_and_unicode_range() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("#\\31 0 { unicode-range: U+4??; }").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Hash("10".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Ident("unicode-range".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::UnicodeRange {
            start: 0x400,
            end: 0x4ff,
        },
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn tokenizes_bad_string_and_url() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer
        .feed("p { content: \"bad\n; background: url(bad(\" ); }")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let expected = vec![
        CssToken::Ident("p".into()),
        CssToken::Whitespace,
        CssToken::CurlyOpen,
        CssToken::Whitespace,
        CssToken::Ident("content".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::BadString,
        CssToken::Whitespace,
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::Ident("background".into()),
        CssToken::Colon,
        CssToken::Whitespace,
        CssToken::BadUrl,
        CssToken::Semicolon,
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}
