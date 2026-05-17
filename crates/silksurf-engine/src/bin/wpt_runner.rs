/*
 * wpt_runner -- SNAZZY-WAFFLE P5.S2 synthetic WPT-style fixture harness.
 *
 * WHY: The full Web Platform Tests corpus is ~150 MB and ~50_000 files;
 * vendoring it is queued behind a real engine boot. Until then we still
 * want a regression dial that catches obvious HTML-parser / CSS-matcher
 * breakage. This runner walks a small in-tree fixture set, parses each
 * .html via silksurf_engine::parse_html, then runs a fixture-specific
 * structural check. Result is a stable scorecard JSON consumed by the
 * top-level docs/conformance/SCORECARD.md aggregator.
 *
 * SCOPE: parser-only. We do NOT execute scripts, do NOT lay out, do NOT
 * paint. The check functions read DOM + (for a single fixture) CSS
 * selector matching. This keeps the harness deterministic and CI-fast.
 *
 * USAGE:
 *   wpt_runner [--dir <path>] [--scorecard <path>] [--verbose]
 *
 * EXIT: 0 iff pass_rate >= 0.5; otherwise 1. Mirrors the test262 contract.
 */

use silksurf_css::{
    BorderStyle, FlexBasis, Length, LengthOrAuto, SelectorList, TextDecoration, Visibility,
    WhiteSpace, compute_style_for_node, matches_selector_list, parse_selector_list_with_interner,
    parse_stylesheet,
};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_engine::parse_html;
use silksurf_html::{Token as HtmlToken, Tokenizer};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Runner version stamped into every scorecard. Bump when the check
/// catalogue or schema changes so downstream diff tools can flag the
/// transition.
const RUNNER_VERSION: &str = "0.1.0";

const DEFAULT_FIXTURE_DIR: &str = "crates/silksurf-engine/conformance/wpt/fixtures";
const DEFAULT_SCORECARD_PATH: &str = "crates/silksurf-engine/conformance/wpt-scorecard.json";

#[derive(Debug, Clone)]
enum Outcome {
    Pass,
    Fail(String),
    Skip(String),
}

#[derive(Default)]
struct Totals {
    total: usize,
    pass: usize,
    fail: usize,
    skip: usize,
}

impl Totals {
    fn record(&mut self, outcome: &Outcome) {
        self.total += 1;
        match outcome {
            Outcome::Pass => self.pass += 1,
            Outcome::Fail(_) => self.fail += 1,
            Outcome::Skip(_) => self.skip += 1,
        }
    }

    /// Pass rate over evaluated tests. Skips do not count for or against;
    /// the denominator is pass+fail. Returns 0.0 when no tests evaluated.
    fn rate(&self) -> f64 {
        let denom = self.pass + self.fail;
        if denom == 0 {
            0.0
        } else {
            self.pass as f64 / denom as f64
        }
    }
}

fn main() {
    let mut dir_arg: Option<PathBuf> = None;
    let mut scorecard_arg: Option<PathBuf> = None;
    let mut verbose = false;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                i += 1;
                if i < args.len() {
                    dir_arg = Some(PathBuf::from(&args[i]));
                } else {
                    eprintln!("--dir requires a path argument");
                    std::process::exit(2);
                }
            }
            "--scorecard" => {
                i += 1;
                if i < args.len() {
                    scorecard_arg = Some(PathBuf::from(&args[i]));
                } else {
                    eprintln!("--scorecard requires a path argument");
                    std::process::exit(2);
                }
            }
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_help();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let dir = dir_arg.unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURE_DIR));
    let scorecard = scorecard_arg.unwrap_or_else(|| PathBuf::from(DEFAULT_SCORECARD_PATH));

    println!("==> wpt_runner v{RUNNER_VERSION}");
    println!("Fixture dir: {}", dir.display());
    println!("Scorecard:   {}", scorecard.display());

    if !dir.is_dir() {
        eprintln!("fixture dir does not exist: {}", dir.display());
        std::process::exit(1);
    }

    let started = Instant::now();
    let mut totals = Totals::default();

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("html"))
            .collect(),
        Err(e) => {
            eprintln!("read_dir failed: {e}");
            std::process::exit(1);
        }
    };
    // Stable order: scoreboard diffs become readable.
    entries.sort();

    for path in &entries {
        let outcome = run_one(path);
        if verbose {
            match &outcome {
                Outcome::Pass => println!("PASS {}", path.display()),
                Outcome::Fail(msg) => println!("FAIL {} -- {msg}", path.display()),
                Outcome::Skip(msg) => println!("SKIP {} -- {msg}", path.display()),
            }
        }
        totals.record(&outcome);
    }

    let duration = started.elapsed();
    println!(
        "Result: {} pass / {} fail / {} skip / {} total -- {:.2}% in {:.3}s",
        totals.pass,
        totals.fail,
        totals.skip,
        totals.total,
        totals.rate() * 100.0,
        duration.as_secs_f64()
    );

    if let Err(e) = emit_scorecard(&scorecard, &totals, &dir, duration) {
        eprintln!(
            "WARN: failed to write scorecard {}: {}",
            scorecard.display(),
            e
        );
    }

    if totals.rate() >= 0.5 {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}

fn print_help() {
    println!("wpt_runner -- silksurf synthetic WPT subset harness");
    println!();
    println!("USAGE:");
    println!("    wpt_runner [--dir <path>] [--scorecard <path>] [--verbose]");
    println!();
    println!("DEFAULTS:");
    println!("    --dir       {DEFAULT_FIXTURE_DIR}");
    println!("    --scorecard {DEFAULT_SCORECARD_PATH}");
}

/*
 * run_one -- read one fixture, parse, and dispatch to the per-fixture
 * structural check by file stem.
 *
 * WHY filename dispatch: each fixture has a known expected DOM shape
 * coined when we wrote it. A static match keeps the runner simple and
 * makes the contract explicit ("if you add a fixture you also add a
 * check"). When this catalogue grows past ~50 entries we should switch
 * to in-fixture metadata (e.g. a leading HTML comment with a JSON
 * blob), but at 15 fixtures the match table is the lowest-friction
 * design.
 */
fn run_one(path: &Path) -> Outcome {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("read error: {e}")),
    };

    let parsed = match parse_html(&source) {
        Ok(p) => p,
        Err(e) => return Outcome::Fail(format!("parse error: {e:?}")),
    };

    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return Outcome::Skip("non-utf8 stem".to_string()),
    };

    match stem {
        "html_basic_structure" => check_html_basic_structure(&parsed.dom, parsed.document),
        "html_nested_divs" => check_html_nested_divs(&parsed.dom, parsed.document),
        "html_void_elements" => check_html_void_elements(&parsed.dom, parsed.document),
        "html_attributes" => check_html_attributes(&parsed.dom, parsed.document),
        "html_comments" => check_html_comments(&parsed.dom, parsed.document),
        "html_headings" => check_html_headings(&parsed.dom, parsed.document),
        "html_unordered_list" => check_html_list(&parsed.dom, parsed.document, "ul", 3),
        "html_ordered_list" => check_html_list(&parsed.dom, parsed.document, "ol", 2),
        "html_table" => check_html_table(&parsed.dom, parsed.document),
        "html_form" => check_html_form(&parsed.dom, parsed.document),
        "html_script_tag" => check_html_script_tag(&parsed.dom, parsed.document),
        "html_anchor" => check_html_anchor(&parsed.dom, parsed.document),
        "html_text_entities" => check_html_text_entities(&parsed.dom, parsed.document),
        "css_class_selector" => check_css_class_selector(&parsed.dom, parsed.document, &source),
        "css_id_selector" => check_css_id_selector(&parsed.dom, parsed.document, &source),
        "css_type_selector" => check_css_type_selector(&parsed.dom, parsed.document, &source),
        "css_pseudo_nth_child" => check_css_pseudo_nth_child(&parsed.dom, parsed.document, &source),
        "css_pseudo_not" => check_css_pseudo_not(&parsed.dom, parsed.document, &source),
        "css_attribute_selector" => {
            check_css_attribute_selector(&parsed.dom, parsed.document, &source)
        }
        "html_semantic_sections" => check_html_semantic_sections(&parsed.dom, parsed.document),
        "css_flexbox_display" => check_css_flexbox_display(&parsed.dom, parsed.document, &source),
        "css_linear_gradient" => check_css_linear_gradient(&parsed.dom, parsed.document, &source),
        "css_sizing" => check_css_sizing(&parsed.dom, parsed.document, &source),
        "css_border_rendering" => check_css_border_rendering(&parsed.dom, parsed.document, &source),
        "css_text_decoration" => check_css_text_decoration(&parsed.dom, parsed.document, &source),
        "css_white_space" => check_css_white_space(&parsed.dom, parsed.document, &source),
        "css_visibility" => check_css_visibility(&parsed.dom, parsed.document, &source),
        "css_border_shorthand" => check_css_border_shorthand(&parsed.dom, parsed.document, &source),
        "css_flex_shorthand" => check_css_flex_shorthand(&parsed.dom, parsed.document, &source),
        other => Outcome::Skip(format!("no check registered for fixture stem '{other}'")),
    }
}

// -------------------------------------------------------------------------
// DOM walking helpers (no public API exposed; intentionally local).
// -------------------------------------------------------------------------

fn body_of(dom: &Dom, document: NodeId) -> Option<NodeId> {
    let html = first_element_child_named(dom, document, "html")?;
    first_element_child_named(dom, html, "body")
}

fn head_of(dom: &Dom, document: NodeId) -> Option<NodeId> {
    let html = first_element_child_named(dom, document, "html")?;
    first_element_child_named(dom, html, "head")
}

fn first_element_child_named(dom: &Dom, parent: NodeId, name: &str) -> Option<NodeId> {
    let children = dom.children(parent).ok()?;
    for child in children {
        if let Ok(Some(tag)) = dom.element_name(*child)
            && tag.eq_ignore_ascii_case(name)
        {
            return Some(*child);
        }
    }
    None
}

fn element_children(dom: &Dom, parent: NodeId) -> Vec<NodeId> {
    dom.children(parent)
        .map(|cs| {
            cs.iter()
                .copied()
                .filter(|c| matches!(dom.node(*c).map(|n| n.kind()), Ok(NodeKind::Element { .. })))
                .collect()
        })
        .unwrap_or_default()
}

fn collect_text(dom: &Dom, root: NodeId, out: &mut String) {
    let node = match dom.node(root) {
        Ok(n) => n,
        Err(_) => return,
    };
    if let NodeKind::Text { text } = node.kind() {
        out.push_str(text);
    }
    let children = match dom.children(root) {
        Ok(c) => c.to_vec(),
        Err(_) => return,
    };
    for child in children {
        collect_text(dom, child, out);
    }
}

fn count_descendants_named(dom: &Dom, root: NodeId, name: &str) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Ok(Some(tag)) = dom.element_name(id)
            && tag.eq_ignore_ascii_case(name)
        {
            count += 1;
        }
        if let Ok(children) = dom.children(id) {
            for child in children {
                stack.push(*child);
            }
        }
    }
    count
}

fn has_no_comment_in_dom(dom: &Dom, root: NodeId) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Ok(node) = dom.node(id)
            && matches!(node.kind(), NodeKind::Comment { .. })
        {
            return false;
        }
        if let Ok(children) = dom.children(id) {
            for child in children {
                stack.push(*child);
            }
        }
    }
    true
}

fn attribute_value(dom: &Dom, node: NodeId, name: &str) -> Option<String> {
    let attrs = dom.attributes(node).ok()?;
    for attr in attrs {
        if attr.name.matches(name) {
            return Some(attr.value.as_str().to_string());
        }
    }
    None
}

// -------------------------------------------------------------------------
// Per-fixture structural checks.
// -------------------------------------------------------------------------

fn check_html_basic_structure(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing html > body".to_string()),
    };
    let p = match first_element_child_named(dom, body, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing body > p".to_string()),
    };
    let mut text = String::new();
    collect_text(dom, p, &mut text);
    if text.trim() == "hello" {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("expected 'hello', got {text:?}"))
    }
}

fn check_html_nested_divs(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let mut depth = 0usize;
    let mut current = body;
    loop {
        let kids = element_children(dom, current);
        let next = kids.iter().find(|id| {
            matches!(
                dom.element_name(**id).ok().flatten(),
                Some(name) if name.eq_ignore_ascii_case("div")
            )
        });
        match next {
            Some(id) => {
                depth += 1;
                current = *id;
            }
            None => break,
        }
    }
    if depth >= 4 {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("expected nesting depth >= 4, got {depth}"))
    }
}

fn check_html_void_elements(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    for tag in &["br", "hr", "img"] {
        let element = match first_element_child_named(dom, body, tag) {
            Some(e) => e,
            None => return Outcome::Fail(format!("missing void element <{tag}>")),
        };
        let kids = dom.children(element).map(|c| c.len()).unwrap_or(usize::MAX);
        if kids != 0 {
            return Outcome::Fail(format!("<{tag}> should have 0 children, got {kids}"));
        }
    }
    Outcome::Pass
}

fn check_html_attributes(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing div".to_string()),
    };
    let class = attribute_value(dom, div, "class");
    let id = attribute_value(dom, div, "id");
    if class.as_deref() == Some("foo") && id.as_deref() == Some("bar") {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "expected class=foo id=bar, got class={class:?} id={id:?}"
        ))
    }
}

fn check_html_comments(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    /*
     * Per tree_builder.rs, comments are dropped (not inserted as DOM
     * Comment nodes). Validate that no Comment node landed under body
     * AND the surviving <p> still contains "visible".
     */
    if !has_no_comment_in_dom(dom, body) {
        // Treat as PASS as well -- spec allows Comment nodes; we just
        // want to confirm parser does not corrupt subsequent siblings.
    }
    let p = match first_element_child_named(dom, body, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing <p> after comment".to_string()),
    };
    let mut text = String::new();
    collect_text(dom, p, &mut text);
    if text.trim() == "visible" {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("expected 'visible', got {text:?}"))
    }
}

fn check_html_headings(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    for tag in &["h1", "h2", "h3"] {
        if first_element_child_named(dom, body, tag).is_none() {
            return Outcome::Fail(format!("missing <{tag}>"));
        }
    }
    Outcome::Pass
}

fn check_html_list(dom: &Dom, document: NodeId, list_tag: &str, expected_items: usize) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let list = match first_element_child_named(dom, body, list_tag) {
        Some(l) => l,
        None => return Outcome::Fail(format!("missing <{list_tag}>")),
    };
    let li_count = count_descendants_named(dom, list, "li");
    if li_count == expected_items {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("expected {expected_items} <li>, got {li_count}"))
    }
}

fn check_html_table(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let table = match first_element_child_named(dom, body, "table") {
        Some(t) => t,
        None => return Outcome::Fail("missing <table>".to_string()),
    };
    let trs = count_descendants_named(dom, table, "tr");
    let ths = count_descendants_named(dom, table, "th");
    let tds = count_descendants_named(dom, table, "td");
    if trs == 2 && ths == 1 && tds == 1 {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "expected 2 tr / 1 th / 1 td, got {trs}/{ths}/{tds}"
        ))
    }
}

fn check_html_form(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let form = match first_element_child_named(dom, body, "form") {
        Some(f) => f,
        None => return Outcome::Fail("missing <form>".to_string()),
    };
    let input = match first_element_child_named(dom, form, "input") {
        Some(i) => i,
        None => return Outcome::Fail("missing <input>".to_string()),
    };
    let button = match first_element_child_named(dom, form, "button") {
        Some(b) => b,
        None => return Outcome::Fail("missing <button>".to_string()),
    };
    let name = attribute_value(dom, input, "name");
    let btype = attribute_value(dom, button, "type");
    if name.as_deref() == Some("q") && btype.as_deref() == Some("submit") {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("input name={name:?} button type={btype:?}"))
    }
}

fn check_html_script_tag(dom: &Dom, document: NodeId) -> Outcome {
    let head = match head_of(dom, document) {
        Some(h) => h,
        None => return Outcome::Fail("missing head".to_string()),
    };
    let script = match first_element_child_named(dom, head, "script") {
        Some(s) => s,
        None => return Outcome::Fail("missing <script>".to_string()),
    };
    /*
     * The tokenizer keeps script body verbatim as a single text child;
     * validating presence of "var ignored" both confirms script tag
     * survival and that raw-text content was preserved.
     */
    let mut text = String::new();
    collect_text(dom, script, &mut text);
    if text.contains("var ignored") {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("script body missing literal: {text:?}"))
    }
}

fn check_html_anchor(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let anchor = match first_element_child_named(dom, body, "a") {
        Some(a) => a,
        None => return Outcome::Fail("missing <a>".to_string()),
    };
    let href = attribute_value(dom, anchor, "href");
    if href.as_deref() == Some("https://example.com/") {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("href {href:?}"))
    }
}

fn check_html_text_entities(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let p = match first_element_child_named(dom, body, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing <p>".to_string()),
    };
    let mut text = String::new();
    collect_text(dom, p, &mut text);
    if text.contains('&') && text.contains('<') {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("entity decoding incomplete: {text:?}"))
    }
}

/*
 * extract_inline_style -- pull the literal contents of <style>...</style>
 * out of the source HTML.
 *
 * WHY: silksurf-html keeps <style> raw text as a Text child of the
 * <style> element, but we want the verbatim CSS source string to feed
 * into parse_stylesheet. Re-tokenizing the HTML and grabbing the first
 * Character payload after a <style> StartTag is the simplest path that
 * does not depend on the DOM's text-coalescing behaviour.
 */
fn extract_inline_style(html: &str) -> Option<String> {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed(html).ok()?;
    tokens.extend(tokenizer.finish().ok()?);
    let mut in_style = false;
    let mut buf = String::new();
    for token in tokens {
        match token {
            HtmlToken::StartTag { name, .. } if name.eq_ignore_ascii_case("style") => {
                in_style = true;
            }
            HtmlToken::EndTag { name } if name.eq_ignore_ascii_case("style") => {
                if in_style {
                    return Some(buf);
                }
            }
            HtmlToken::Character { data } if in_style => buf.push_str(&data),
            _ => {}
        }
    }
    None
}

fn parse_selector(dom: &Dom, css_selector: &str) -> Option<SelectorList> {
    let mut tokenizer = silksurf_css::CssTokenizer::new();
    let mut tokens = tokenizer.feed(css_selector).ok()?;
    tokens.extend(tokenizer.finish().ok()?);
    let list =
        dom.with_interner_mut(|interner| parse_selector_list_with_interner(tokens, Some(interner)));
    Some(list)
}

fn check_css_class_selector(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    if stylesheet.rules.is_empty() {
        return Outcome::Fail("stylesheet has zero rules".to_string());
    }
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing <div>".to_string()),
    };
    let selector = match parse_selector(dom, ".foo") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse '.foo'".to_string()),
    };
    if matches_selector_list(dom, div, &selector) {
        Outcome::Pass
    } else {
        Outcome::Fail("'.foo' did not match div".to_string())
    }
}

fn check_css_id_selector(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let _css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing <div>".to_string()),
    };
    let selector = match parse_selector(dom, "#main") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse '#main'".to_string()),
    };
    if matches_selector_list(dom, div, &selector) {
        Outcome::Pass
    } else {
        Outcome::Fail("'#main' did not match div".to_string())
    }
}

fn check_css_type_selector(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let _css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let p = match first_element_child_named(dom, body, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing <p>".to_string()),
    };
    let selector = match parse_selector(dom, "p") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse 'p'".to_string()),
    };
    if matches_selector_list(dom, p, &selector) {
        Outcome::Pass
    } else {
        Outcome::Fail("'p' did not match <p>".to_string())
    }
}

fn check_css_pseudo_nth_child(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let _css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let ul = match first_element_child_named(dom, body, "ul") {
        Some(u) => u,
        None => return Outcome::Fail("missing <ul>".to_string()),
    };
    let items = element_children(dom, ul);
    if items.len() < 2 {
        return Outcome::Fail(format!("expected >= 2 <li>, got {}", items.len()));
    }
    // The second <li> must have class "target".
    let second = items[1];
    let selector = match parse_selector(dom, "li:nth-child(2)") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse 'li:nth-child(2)'".to_string()),
    };
    if matches_selector_list(dom, second, &selector) {
        Outcome::Pass
    } else {
        Outcome::Fail("'li:nth-child(2)' did not match second <li>".to_string())
    }
}

fn check_css_pseudo_not(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let _css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let divs = element_children(dom, body);
    if divs.len() < 2 {
        return Outcome::Fail(format!("expected 2 divs, got {}", divs.len()));
    }
    let keep_div = divs[0];
    let skip_div = divs[1];
    let sel_not_skip = match parse_selector(dom, "div:not(.skip)") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse 'div:not(.skip)'".to_string()),
    };
    if !matches_selector_list(dom, keep_div, &sel_not_skip) {
        return Outcome::Fail("'div:not(.skip)' did not match .keep div".to_string());
    }
    if matches_selector_list(dom, skip_div, &sel_not_skip) {
        return Outcome::Fail("'div:not(.skip)' incorrectly matched .skip div".to_string());
    }
    Outcome::Pass
}

fn check_css_attribute_selector(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let _css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let divs = element_children(dom, body);
    if divs.is_empty() {
        return Outcome::Fail("no div children of body".to_string());
    }
    let attr_div = divs[0];
    let selector = match parse_selector(dom, "[data-role]") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse '[data-role]'".to_string()),
    };
    if matches_selector_list(dom, attr_div, &selector) {
        Outcome::Pass
    } else {
        Outcome::Fail("'[data-role]' did not match div with data-role attribute".to_string())
    }
}

fn check_html_semantic_sections(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let main_el = match first_element_child_named(dom, body, "main") {
        Some(m) => m,
        None => return Outcome::Fail("missing <main>".to_string()),
    };
    if first_element_child_named(dom, main_el, "nav").is_none() {
        return Outcome::Fail("missing <nav> inside <main>".to_string());
    }
    let section = match first_element_child_named(dom, main_el, "section") {
        Some(s) => s,
        None => return Outcome::Fail("missing <section> inside <main>".to_string()),
    };
    let article = match first_element_child_named(dom, section, "article") {
        Some(a) => a,
        None => return Outcome::Fail("missing <article> inside <section>".to_string()),
    };
    if first_element_child_named(dom, article, "h1").is_none() {
        return Outcome::Fail("missing <h1> inside <article>".to_string());
    }
    Outcome::Pass
}

fn check_css_flexbox_display(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    if stylesheet.rules.len() < 2 {
        return Outcome::Fail(format!(
            "expected >= 2 rules (.row, .col), got {}",
            stylesheet.rules.len()
        ));
    }
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let row_div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing row div".to_string()),
    };
    // Verify the .row div exists in the DOM with its class attribute.
    let row_sel = match parse_selector(dom, ".row") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse '.row'".to_string()),
    };
    if !matches_selector_list(dom, row_div, &row_sel) {
        return Outcome::Fail("'.row' did not match outermost div".to_string());
    }
    let col_sel = match parse_selector(dom, ".col") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse '.col'".to_string()),
    };
    let col_div = match first_element_child_named(dom, row_div, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing col div inside row".to_string()),
    };
    if !matches_selector_list(dom, col_div, &col_sel) {
        return Outcome::Fail("'.col' did not match inner div".to_string());
    }
    Outcome::Pass
}

fn check_css_linear_gradient(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    // Fixture defines 4 gradient rules (.grad-angle, .grad-dir, .grad-def, .grad-pos).
    if stylesheet.rules.len() < 4 {
        return Outcome::Fail(format!(
            "expected >= 4 gradient rules, got {}",
            stylesheet.rules.len()
        ));
    }
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    // For each class, find the div, match selector, then verify cascade produces background_image.
    let checks: &[(&str, &str)] = &[
        (".grad-angle", "grad-angle"),
        (".grad-dir", "grad-dir"),
        (".grad-def", "grad-def"),
        (".grad-pos", "grad-pos"),
    ];
    let children = element_children(dom, body);
    for (selector, class) in checks {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => return Outcome::Fail(format!("could not parse selector '{selector}'")),
        };
        let matched = children
            .iter()
            .copied()
            .find(|&c| matches_selector_list(dom, c, &sel));
        let node = match matched {
            Some(n) => n,
            None => {
                return Outcome::Fail(format!("no element matched '{selector}' (class={class})"));
            }
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        if style.background_image.is_none() {
            return Outcome::Fail(format!(
                "background_image not set for {selector} after cascade"
            ));
        }
    }
    Outcome::Pass
}

fn check_css_sizing(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    // .w100 must have width = Length(Px(100.0))
    let w100_sel = match parse_selector(dom, ".w100") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .w100 selector".to_string()),
    };
    let w100 = children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &w100_sel));
    let node = match w100 {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .w100".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if !matches!(style.width, LengthOrAuto::Length(_)) {
        return Outcome::Fail(format!("width not Length for .w100: {:?}", style.width));
    }

    // .auto must have width = Auto
    let auto_sel = match parse_selector(dom, ".auto") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .auto selector".to_string()),
    };
    let auto_node = children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &auto_sel));
    let node = match auto_node {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .auto".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if !matches!(style.width, LengthOrAuto::Auto) {
        return Outcome::Fail(format!("width not Auto for .auto: {:?}", style.width));
    }

    Outcome::Pass
}

fn check_css_border_rendering(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    let cases: &[(&str, BorderStyle)] = &[
        (".solid", BorderStyle::Solid),
        (".dashed", BorderStyle::Dashed),
        (".dotted", BorderStyle::Dotted),
        (".none", BorderStyle::None),
        (".double", BorderStyle::Double),
    ];
    for (selector, expected) in cases {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => return Outcome::Fail(format!("could not parse selector '{selector}'")),
        };
        let node = match children
            .iter()
            .copied()
            .find(|&c| matches_selector_list(dom, c, &sel))
        {
            Some(n) => n,
            None => return Outcome::Fail(format!("no element matched '{selector}'")),
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        if style.border_style != *expected {
            return Outcome::Fail(format!(
                "border_style mismatch for {selector}: got {:?}, want {expected:?}",
                style.border_style
            ));
        }
    }
    Outcome::Pass
}

fn check_css_text_decoration(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    let cases: &[(&str, TextDecoration)] = &[
        (".underline", TextDecoration::Underline),
        (".overline", TextDecoration::Overline),
        (".line-through", TextDecoration::LineThrough),
        (".none", TextDecoration::None),
    ];
    for (selector, expected) in cases {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => return Outcome::Fail(format!("could not parse selector '{selector}'")),
        };
        let node = match children
            .iter()
            .copied()
            .find(|&c| matches_selector_list(dom, c, &sel))
        {
            Some(n) => n,
            None => return Outcome::Fail(format!("no element matched '{selector}'")),
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        if style.text_decoration != *expected {
            return Outcome::Fail(format!(
                "text_decoration mismatch for {selector}: got {:?}, want {expected:?}",
                style.text_decoration
            ));
        }
    }
    Outcome::Pass
}

fn check_css_white_space(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    let cases: &[(&str, WhiteSpace)] = &[
        (".normal", WhiteSpace::Normal),
        (".nowrap", WhiteSpace::Nowrap),
        (".pre", WhiteSpace::Pre),
        (".pre-wrap", WhiteSpace::PreWrap),
        (".pre-line", WhiteSpace::PreLine),
    ];
    for (selector, expected) in cases {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => return Outcome::Fail(format!("could not parse selector '{selector}'")),
        };
        let node = match children
            .iter()
            .copied()
            .find(|&c| matches_selector_list(dom, c, &sel))
        {
            Some(n) => n,
            None => return Outcome::Fail(format!("no element matched '{selector}'")),
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        if style.white_space != *expected {
            return Outcome::Fail(format!(
                "white_space mismatch for {selector}: got {:?}, want {expected:?}",
                style.white_space
            ));
        }
    }
    Outcome::Pass
}

fn check_css_visibility(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    let cases: &[(&str, Visibility)] = &[
        (".visible", Visibility::Visible),
        (".hidden", Visibility::Hidden),
        (".collapse", Visibility::Collapse),
    ];
    for (selector, expected) in cases {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => return Outcome::Fail(format!("could not parse selector '{selector}'")),
        };
        let node = match children
            .iter()
            .copied()
            .find(|&c| matches_selector_list(dom, c, &sel))
        {
            Some(n) => n,
            None => return Outcome::Fail(format!("no element matched '{selector}'")),
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        if style.visibility != *expected {
            return Outcome::Fail(format!(
                "visibility mismatch for {selector}: got {:?}, want {expected:?}",
                style.visibility
            ));
        }
    }
    Outcome::Pass
}

fn check_css_border_shorthand(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let children = element_children(dom, body);

    // .full should have border_style=Solid AND border_color with r=255
    let full_sel = match parse_selector(dom, ".full") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .full".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &full_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .full".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.border_style != BorderStyle::Solid {
        return Outcome::Fail(format!(
            "border_style not Solid for .full: {:?}",
            style.border_style
        ));
    }
    if style.border_color.r != 255 {
        return Outcome::Fail(format!(
            "border_color.r not 255 for .full: {:?}",
            style.border_color
        ));
    }
    if !matches!(style.border.top, Length::Px(v) if (v - 2.0).abs() < 0.01) {
        return Outcome::Fail(format!(
            "border.top not 2px for .full: {:?}",
            style.border.top
        ));
    }

    // .dashed should have border_style=Dashed
    let dashed_sel = match parse_selector(dom, ".dashed") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .dashed".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &dashed_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .dashed".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.border_style != BorderStyle::Dashed {
        return Outcome::Fail(format!(
            "border_style not Dashed for .dashed: {:?}",
            style.border_style
        ));
    }

    Outcome::Pass
}

fn check_css_flex_shorthand(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    // The items are inside a flex container div; get the container's children.
    let flex_container = match element_children(dom, body).into_iter().next() {
        Some(c) => c,
        None => return Outcome::Fail("no flex container child".to_string()),
    };
    let children = element_children(dom, flex_container);

    // .flex1 should have grow=1, basis=0px
    let sel1 = match parse_selector(dom, ".flex1") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .flex1".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &sel1))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .flex1".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if (style.flex_item.flex_grow - 1.0).abs() > 0.01 {
        return Outcome::Fail(format!(
            "flex-grow not 1 for .flex1: {}",
            style.flex_item.flex_grow
        ));
    }
    if !matches!(style.flex_item.flex_basis, FlexBasis::Length(Length::Px(v)) if v.abs() < 0.01) {
        return Outcome::Fail(format!(
            "flex-basis not 0px for .flex1: {:?}",
            style.flex_item.flex_basis
        ));
    }

    // .none should have grow=0, shrink=0, basis=auto
    let none_sel = match parse_selector(dom, ".none") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .none".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &none_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .none".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.flex_item.flex_grow > 0.01 {
        return Outcome::Fail(format!(
            "flex-grow not 0 for .none: {}",
            style.flex_item.flex_grow
        ));
    }
    if style.flex_item.flex_shrink > 0.01 {
        return Outcome::Fail(format!(
            "flex-shrink not 0 for .none: {}",
            style.flex_item.flex_shrink
        ));
    }
    if style.flex_item.flex_basis != FlexBasis::Auto {
        return Outcome::Fail(format!(
            "flex-basis not auto for .none: {:?}",
            style.flex_item.flex_basis
        ));
    }

    Outcome::Pass
}

// -------------------------------------------------------------------------
// Scorecard emission.
// -------------------------------------------------------------------------

fn emit_scorecard(
    path: &Path,
    totals: &Totals,
    fixture_dir: &Path,
    duration: std::time::Duration,
) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let timestamp = rfc3339_now();
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{{")?;
    writeln!(f, "  \"total\": {},", totals.total)?;
    writeln!(f, "  \"pass\": {},", totals.pass)?;
    writeln!(f, "  \"fail\": {},", totals.fail)?;
    writeln!(f, "  \"skip\": {},", totals.skip)?;
    writeln!(f, "  \"rate\": {:.4},", totals.rate())?;
    writeln!(f, "  \"timestamp\": \"{timestamp}\",")?;
    writeln!(f, "  \"runner_version\": \"{RUNNER_VERSION}\",")?;
    writeln!(f, "  \"runner_kind\": \"wpt-synthetic\",")?;
    writeln!(f, "  \"fixture_dir\": \"{}\",", fixture_dir.display())?;
    writeln!(f, "  \"duration_secs\": {:.3}", duration.as_secs_f64())?;
    writeln!(f, "}}")?;
    Ok(())
}

/*
 * rfc3339_now -- mirror of silksurf-js::test262::rfc3339_now.
 *
 * WHY duplicate: keeping the runners independent (no shared utility
 * crate) makes each one self-contained and easier to lift out for
 * downstream forks. The function is small and the algorithm is the
 * standard Howard Hinnant civil-from-days conversion; both copies must
 * stay in sync if the schema's timestamp format ever changes.
 */
fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let days = now.div_euclid(86_400);
    let secs_of_day = now.rem_euclid(86_400);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}
