//! Regex helpers backing `String.prototype.match`/`search` and `RegExp`.
//!
//! All pattern strings are compiled as ECMAScript regexes via the `regress`
//! crate. On a pattern syntax error the methods return Null/-1 rather than
//! throwing `SyntaxError`; proper error propagation is deferred to the full
//! `RegExp` constructor (future work).

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::vm::value::{Object, PropertyKey, Value};

/// Run `String.prototype.match(pattern)` semantics.
///
/// When pattern is a plain string (not a `RegExp` object) JS compiles it as a
/// regex and returns an Array with the first match and capture groups plus
/// `index` and `input` properties, or Null on no match.
#[must_use]
pub fn regex_match(text: &str, pattern: &str) -> Value {
    let Ok(re) = Regex::new(pattern) else {
        return Value::Null;
    };
    match re.find(text) {
        None => Value::Null,
        Some(m) => build_match_result(text, &m),
    }
}

/// Run `String.prototype.search(pattern)` semantics.
///
/// Returns the char-index (not byte-index) of the first match, or -1.
#[must_use]
pub fn regex_search(text: &str, pattern: &str) -> Value {
    let Ok(re) = Regex::new(pattern) else {
        return Value::Number(-1.0);
    };
    match re.find(text) {
        None => Value::Number(-1.0),
        Some(m) => {
            let char_idx = text[..m.range().start].chars().count();
            Value::Number(char_idx as f64)
        }
    }
}

/// Build the JS Array match-result object.
///
/// `result[0]`    = full match text
/// `result[1..]`  = capture groups (Undefined when a group did not participate)
/// `result.index` = char-index of match start in input
/// `result.input` = original input string
fn build_match_result(text: &str, m: &regress::Match) -> Value {
    let obj_rc = Rc::new(RefCell::new(Object::new()));
    {
        let mut o = obj_rc.borrow_mut();

        // result[0]: full match
        let full = &text[m.range()];
        o.set_by_key(PropertyKey::Index(0), Value::string(full));

        // result[1..]: capture groups (captures[i] = group i+1 in the regex)
        for (i, cap) in m.captures.iter().enumerate() {
            let val = match cap {
                Some(r) => Value::string(&text[r.clone()]),
                None => Value::Undefined,
            };
            o.set_by_key(PropertyKey::Index((i + 1) as u32), val);
        }

        let total = 1 + m.captures.len();
        o.set_by_str("length", Value::Number(total as f64));

        // index: char offset of match start
        let char_idx = text[..m.range().start].chars().count();
        o.set_by_str("index", Value::Number(char_idx as f64));

        // input: original string
        o.set_by_str("input", Value::string(text));
    }
    Value::Object(obj_rc)
}
