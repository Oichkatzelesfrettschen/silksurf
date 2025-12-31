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
    let mut tokens = tokenizer
        .feed("#id { /* note */ padding: 10px }")
        .unwrap();
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
        CssToken::Number("10".into()),
        CssToken::Ident("px".into()),
        CssToken::Whitespace,
        CssToken::CurlyClose,
        CssToken::Eof,
    ];

    assert_eq!(tokens, expected);
}
