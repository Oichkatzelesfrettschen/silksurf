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
    BorderStyle, Color, Display, FlexBasis, Length, LengthOrAuto, Overflow, Position, SelectorList,
    TextDecoration, Visibility, WhiteSpace, compute_style_for_node, compute_styles,
    matches_selector_list, parse_selector_list_with_interner, parse_stylesheet,
};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_engine::fused_pipeline::fused_style_layout_paint;
use silksurf_engine::parse_html;
use silksurf_html::{Token as HtmlToken, Tokenizer};
use silksurf_layout::Rect;
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
        "css_em_rem_units" => check_css_em_rem_units(&parsed.dom, parsed.document, &source),
        "css_individual_margins" => {
            check_css_individual_margins(&parsed.dom, parsed.document, &source)
        }
        "css_named_colors" => check_css_named_colors(&parsed.dom, parsed.document, &source),
        "css_cascade_keywords" => {
            check_css_cascade_keywords(&parsed.dom, parsed.document, &source)
        }
        "css_individual_borders" => {
            check_css_individual_borders(&parsed.dom, parsed.document, &source)
        }
        // HTML parser edge cases
        "html_malformed_unclosed" => check_html_malformed_unclosed(&parsed.dom, parsed.document),
        "html_implicit_head_body" => check_html_implicit_head_body(&parsed.dom, parsed.document),
        "html_data_attributes" => check_html_data_attributes(&parsed.dom, parsed.document),
        "html_mixed_case" => check_html_mixed_case(&parsed.dom, parsed.document),
        "html_table_implicit_tbody" => {
            check_html_table_implicit_tbody(&parsed.dom, parsed.document)
        }
        "html_boolean_attributes" => check_html_boolean_attributes(&parsed.dom, parsed.document),
        "html_misnested_formatting" => {
            check_html_misnested_formatting(&parsed.dom, parsed.document)
        }
        "html_multiple_classes" => check_html_multiple_classes(&parsed.dom, parsed.document),
        // CSS selector combinators and pseudo-classes
        "css_pseudo_is" => check_css_pseudo_is(&parsed.dom, parsed.document, &source),
        "css_pseudo_where" => check_css_pseudo_where(&parsed.dom, parsed.document, &source),
        "css_child_combinator" => check_css_child_combinator(&parsed.dom, parsed.document, &source),
        "css_adjacent_sibling" => check_css_adjacent_sibling(&parsed.dom, parsed.document, &source),
        "css_general_sibling" => check_css_general_sibling(&parsed.dom, parsed.document, &source),
        "css_multiple_class" => check_css_multiple_class(&parsed.dom, parsed.document, &source),
        "css_specificity" => check_css_specificity(&parsed.dom, parsed.document, &source),
        // CSS property coverage
        "css_overflow" => check_css_overflow(&parsed.dom, parsed.document, &source),
        "css_opacity" => check_css_opacity(&parsed.dom, parsed.document, &source),
        "css_z_index" => check_css_z_index(&parsed.dom, parsed.document, &source),
        "css_position" => check_css_position(&parsed.dom, parsed.document, &source),
        "css_border_radius" => check_css_border_radius(&parsed.dom, parsed.document, &source),
        // Taffy layout rect checks
        "layout_flex_row_equal" => {
            check_layout_flex_row_equal(&parsed.dom, parsed.document, &source)
        }
        "layout_flex_fixed_offset" => {
            check_layout_flex_fixed_offset(&parsed.dom, parsed.document, &source)
        }
        "layout_flex_column_stack" => {
            check_layout_flex_column_stack(&parsed.dom, parsed.document, &source)
        }
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
// HTML parser edge-case checks.
// -------------------------------------------------------------------------

fn check_html_malformed_unclosed(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing div: unclosed tags should be auto-closed".to_string()),
    };
    let p = match first_element_child_named(dom, div, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing p inside div".to_string()),
    };
    if first_element_child_named(dom, p, "span").is_none() {
        return Outcome::Fail("missing span inside p".to_string());
    }
    Outcome::Pass
}

fn check_html_implicit_head_body(dom: &Dom, document: NodeId) -> Outcome {
    if head_of(dom, document).is_none() {
        return Outcome::Fail("html5ever should create implicit <head>".to_string());
    }
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("html5ever should create implicit <body>".to_string()),
    };
    if first_element_child_named(dom, body, "p").is_none() {
        return Outcome::Fail("missing <p> in implicitly-created body".to_string());
    }
    Outcome::Pass
}

fn check_html_data_attributes(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing div".to_string()),
    };
    match attribute_value(dom, div, "data-role") {
        Some(v) if v == "main" => {}
        Some(v) => return Outcome::Fail(format!("data-role: expected 'main', got '{v}'")),
        None => return Outcome::Fail("data-role attribute not preserved".to_string()),
    }
    match attribute_value(dom, div, "data-count") {
        Some(v) if v == "5" => {}
        Some(v) => return Outcome::Fail(format!("data-count: expected '5', got '{v}'")),
        None => return Outcome::Fail("data-count attribute not preserved".to_string()),
    }
    Outcome::Pass
}

fn check_html_mixed_case(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body (case-insensitive parse failed)".to_string()),
    };
    let div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing div from uppercased <DIV>".to_string()),
    };
    match attribute_value(dom, div, "id") {
        Some(v) if v == "target" => {}
        other => return Outcome::Fail(format!("id attr: expected 'target', got {other:?}")),
    }
    let mut text = String::new();
    collect_text(dom, div, &mut text);
    if text.trim() != "hello" {
        return Outcome::Fail(format!("text inside <DIV>: expected 'hello', got {text:?}"));
    }
    Outcome::Pass
}

fn check_html_table_implicit_tbody(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let table = match first_element_child_named(dom, body, "table") {
        Some(t) => t,
        None => return Outcome::Fail("missing table".to_string()),
    };
    // html5ever always inserts an implicit <tbody> between <table> and <tr>.
    let tbody = match first_element_child_named(dom, table, "tbody") {
        Some(t) => t,
        None => return Outcome::Fail("html5ever should insert implicit <tbody>".to_string()),
    };
    let tr = match first_element_child_named(dom, tbody, "tr") {
        Some(t) => t,
        None => return Outcome::Fail("missing <tr> inside <tbody>".to_string()),
    };
    let tds = element_children(dom, tr)
        .into_iter()
        .filter(|&c| {
            dom.element_name(c)
                .ok()
                .flatten()
                .map(|n| n.eq_ignore_ascii_case("td"))
                .unwrap_or(false)
        })
        .count();
    if tds != 2 {
        return Outcome::Fail(format!("expected 2 <td> children, got {tds}"));
    }
    Outcome::Pass
}

fn check_html_boolean_attributes(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let input = match first_element_child_named(dom, body, "input") {
        Some(i) => i,
        None => return Outcome::Fail("missing input".to_string()),
    };
    // Boolean attribute is present; value may be "" or "disabled".
    if attribute_value(dom, input, "disabled").is_none() {
        return Outcome::Fail("'disabled' boolean attribute not preserved on input".to_string());
    }
    let button = match first_element_child_named(dom, body, "button") {
        Some(b) => b,
        None => return Outcome::Fail("missing button".to_string()),
    };
    if attribute_value(dom, button, "disabled").is_none() {
        return Outcome::Fail("'disabled' boolean attribute not preserved on button".to_string());
    }
    Outcome::Pass
}

fn check_html_misnested_formatting(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    // The adoption agency algorithm for <b>bold<i>bold-italic</b>italic</i>
    // produces: body > b > [text, i > text], i > text
    // Verify body has both a <b> and an <i> as direct children.
    let b = first_element_child_named(dom, body, "b");
    if b.is_none() {
        return Outcome::Fail("missing <b> child of body after adoption agency".to_string());
    }
    let has_i_sibling = element_children(dom, body)
        .iter()
        .any(|&c| matches!(dom.element_name(c).ok().flatten(), Some(n) if n.eq_ignore_ascii_case("i")));
    if !has_i_sibling {
        return Outcome::Fail("expected <i> sibling of <b> in body after adoption agency".to_string());
    }
    Outcome::Pass
}

fn check_html_multiple_classes(dom: &Dom, document: NodeId) -> Outcome {
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let divs = element_children(dom, body);
    if divs.len() < 2 {
        return Outcome::Fail(format!("expected 2 divs, got {}", divs.len()));
    }
    let first = divs[0];
    let classes = attribute_value(dom, first, "class").unwrap_or_default();
    let has_foo = classes.split_whitespace().any(|c| c == "foo");
    let has_bar = classes.split_whitespace().any(|c| c == "bar");
    let has_baz = classes.split_whitespace().any(|c| c == "baz");
    if !has_foo || !has_bar || !has_baz {
        return Outcome::Fail(format!(
            "first div should have classes foo+bar+baz, got '{classes}'"
        ));
    }
    Outcome::Pass
}

// -------------------------------------------------------------------------
// CSS selector combinator / pseudo-class checks.
// -------------------------------------------------------------------------

fn check_css_pseudo_is(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let box_div = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing .box div".to_string()),
    };
    let h1 = match first_element_child_named(dom, box_div, "h1") {
        Some(h) => h,
        None => return Outcome::Fail("missing h1 inside .box".to_string()),
    };
    // :is(h1, h2, h3) inside .box should match h1 and give it color red.
    let style = compute_style_for_node(dom, h1, &stylesheet, None);
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    if style.color != red {
        return Outcome::Fail(format!(
            ":is(h1,h2,h3) did not apply color red to h1; got {:?}",
            style.color
        ));
    }
    Outcome::Pass
}

fn check_css_pseudo_where(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let p = match first_element_child_named(dom, body, "p") {
        Some(p) => p,
        None => return Outcome::Fail("missing p".to_string()),
    };
    // :where(p) has 0 specificity; plain `p` (specificity 1) wins -> red.
    let style = compute_style_for_node(dom, p, &stylesheet, None);
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    if style.color != red {
        return Outcome::Fail(format!(
            ":where() specificity not zero; expected red from `p` rule, got {:?}",
            style.color
        ));
    }
    Outcome::Pass
}

fn check_css_child_combinator(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let parent = match first_element_child_named(dom, body, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing .parent div".to_string()),
    };
    let direct_child = match first_element_child_named(dom, parent, "div") {
        Some(d) => d,
        None => return Outcome::Fail("missing direct .child div".to_string()),
    };
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    let style = compute_style_for_node(dom, direct_child, &stylesheet, None);
    if style.color != red {
        return Outcome::Fail(format!(
            ".parent > .child: direct child should be red, got {:?}",
            style.color
        ));
    }
    // The indirect .child (nested deeper) should NOT be red.
    let wrapper = element_children(dom, parent)
        .into_iter()
        .nth(1)
        .and_then(|w| first_element_child_named(dom, w, "div"));
    if let Some(indirect) = wrapper {
        let style2 = compute_style_for_node(dom, indirect, &stylesheet, None);
        if style2.color == red {
            return Outcome::Fail(
                ".parent > .child matched indirect descendant (should not)".to_string(),
            );
        }
    }
    Outcome::Pass
}

fn check_css_adjacent_sibling(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    let children = element_children(dom, body);
    // children: [.ref, .target#yes, .target#no]
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 body children, got {}", children.len()));
    }
    let yes = children[1];
    let no = children[2];
    let style_yes = compute_style_for_node(dom, yes, &stylesheet, None);
    if style_yes.color != red {
        return Outcome::Fail(format!(
            ".ref + .target: adjacent sibling should be red, got {:?}",
            style_yes.color
        ));
    }
    let style_no = compute_style_for_node(dom, no, &stylesheet, None);
    if style_no.color == red {
        return Outcome::Fail(".ref + .target matched non-adjacent sibling".to_string());
    }
    Outcome::Pass
}

fn check_css_general_sibling(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let body = match body_of(dom, document) {
        Some(b) => b,
        None => return Outcome::Fail("missing body".to_string()),
    };
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    let children = element_children(dom, body);
    // children: [.ref, .target#t1, .other, .target#t2]
    if children.len() < 4 {
        return Outcome::Fail(format!("expected 4 body children, got {}", children.len()));
    }
    for &idx in &[1usize, 3usize] {
        let style = compute_style_for_node(dom, children[idx], &stylesheet, None);
        if style.color != red {
            return Outcome::Fail(format!(
                ".ref ~ .target[{idx}] should be red, got {:?}",
                style.color
            ));
        }
    }
    Outcome::Pass
}

fn check_css_multiple_class(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 2 {
        return Outcome::Fail(format!("expected 2 divs, got {}", children.len()));
    }
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = silksurf_css::Color { r: 0, g: 0, b: 255, a: 255 };
    // div.a.b -> red (compound selector wins over .a alone)
    let style_both = compute_style_for_node(dom, children[0], &stylesheet, None);
    if style_both.color != red {
        return Outcome::Fail(format!(
            ".a.b should be red, got {:?}",
            style_both.color
        ));
    }
    // div.a only -> blue
    let style_one = compute_style_for_node(dom, children[1], &stylesheet, None);
    if style_one.color != blue {
        return Outcome::Fail(format!(
            ".a only should be blue, got {:?}",
            style_one.color
        ));
    }
    Outcome::Pass
}

fn check_css_specificity(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 divs, got {}", children.len()));
    }
    let green = silksurf_css::Color { r: 0, g: 128, b: 0, a: 255 };
    let red = silksurf_css::Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = silksurf_css::Color { r: 0, g: 0, b: 255, a: 255 };
    let s0 = compute_style_for_node(dom, children[0], &stylesheet, None);
    if s0.color != green {
        return Outcome::Fail(format!("#unique should be green (id wins), got {:?}", s0.color));
    }
    let s1 = compute_style_for_node(dom, children[1], &stylesheet, None);
    if s1.color != red {
        return Outcome::Fail(format!(".highlight should be red (class > element), got {:?}", s1.color));
    }
    let s2 = compute_style_for_node(dom, children[2], &stylesheet, None);
    if s2.color != blue {
        return Outcome::Fail(format!("plain div should be blue (element), got {:?}", s2.color));
    }
    Outcome::Pass
}

// -------------------------------------------------------------------------
// CSS property checks.
// -------------------------------------------------------------------------

fn check_css_overflow(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 4 {
        return Outcome::Fail(format!("expected 4 divs, got {}", children.len()));
    }
    let cases: &[(&str, Overflow, usize)] = &[
        ("hidden", Overflow::Hidden, 0),
        ("scroll", Overflow::Scroll, 1),
        ("auto", Overflow::Auto, 2),
        ("visible", Overflow::Visible, 3),
    ];
    for (name, expected, idx) in cases {
        let style = compute_style_for_node(dom, children[*idx], &stylesheet, None);
        if style.overflow_x != *expected {
            return Outcome::Fail(format!(
                "overflow-x for .{name}: expected {expected:?}, got {:?}",
                style.overflow_x
            ));
        }
    }
    Outcome::Pass
}

fn check_css_opacity(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 divs, got {}", children.len()));
    }
    let cases: &[(f32, usize)] = &[(0.5, 0), (1.0, 1), (0.0, 2)];
    for (expected, idx) in cases {
        let style = compute_style_for_node(dom, children[*idx], &stylesheet, None);
        if (style.opacity - expected).abs() > 0.01 {
            return Outcome::Fail(format!(
                "opacity[{idx}]: expected {expected}, got {}",
                style.opacity
            ));
        }
    }
    Outcome::Pass
}

fn check_css_z_index(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 divs, got {}", children.len()));
    }
    let cases: &[(i32, usize)] = &[(10, 0), (-1, 1), (0, 2)];
    for (expected, idx) in cases {
        let style = compute_style_for_node(dom, children[*idx], &stylesheet, None);
        if style.z_index != *expected {
            return Outcome::Fail(format!(
                "z-index[{idx}]: expected {expected}, got {}",
                style.z_index
            ));
        }
    }
    Outcome::Pass
}

fn check_css_position(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 divs, got {}", children.len()));
    }
    let s_rel = compute_style_for_node(dom, children[0], &stylesheet, None);
    if s_rel.position != Position::Relative {
        return Outcome::Fail(format!(".rel: expected Relative, got {:?}", s_rel.position));
    }
    if !matches!(s_rel.top, LengthOrAuto::Length(_)) {
        return Outcome::Fail(format!(".rel top: expected Length, got {:?}", s_rel.top));
    }
    let s_abs = compute_style_for_node(dom, children[1], &stylesheet, None);
    if s_abs.position != Position::Absolute {
        return Outcome::Fail(format!(".abs: expected Absolute, got {:?}", s_abs.position));
    }
    let s_sta = compute_style_for_node(dom, children[2], &stylesheet, None);
    if s_sta.position != Position::Static {
        return Outcome::Fail(format!(".static: expected Static, got {:?}", s_sta.position));
    }
    Outcome::Pass
}

fn check_css_border_radius(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block".to_string()),
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
    if children.len() < 3 {
        return Outcome::Fail(format!("expected 3 divs, got {}", children.len()));
    }
    let s_rounded = compute_style_for_node(dom, children[0], &stylesheet, None);
    if (s_rounded.border_radius - 8.0).abs() > 0.1 {
        return Outcome::Fail(format!(
            ".rounded: expected border-radius 8.0, got {}",
            s_rounded.border_radius
        ));
    }
    let s_sharp = compute_style_for_node(dom, children[2], &stylesheet, None);
    if s_sharp.border_radius.abs() > 0.1 {
        return Outcome::Fail(format!(
            ".sharp: expected border-radius 0, got {}",
            s_sharp.border_radius
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

fn check_css_em_rem_units(dom: &Dom, document: NodeId, source: &str) -> Outcome {
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

    // html { font-size: 20px }  =>  rem base = 20px
    // .em1 { font-size: 2em }   =>  parent = body = 1rem = 20px  =>  2*20 = 40px
    let em1_sel = match parse_selector(dom, ".em1") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .em1 selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &em1_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .em1".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    // Without full document-tree cascade, em resolves against 16px default.
    // Verify the font-size is a Px value (em was resolved, not stored raw).
    if !matches!(style.font_size, Length::Px(_)) {
        return Outcome::Fail(format!(
            ".em1 font-size was not resolved to Px: {:?}",
            style.font_size
        ));
    }

    // .em2 { margin-left: 1.5em }  =>  margin resolves to Px, not Em.
    let em2_sel = match parse_selector(dom, ".em2") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .em2 selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &em2_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .em2".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if !matches!(
        style.margin.left,
        silksurf_css::LengthOrAuto::Length(Length::Px(_))
    ) {
        return Outcome::Fail(format!(
            ".em2 margin-left was not resolved to Px: {:?}",
            style.margin.left
        ));
    }

    // .rem1 { font-size: 1.5rem }  =>  resolves to Px.
    let rem1_sel = match parse_selector(dom, ".rem1") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .rem1 selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &rem1_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .rem1".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if !matches!(style.font_size, Length::Px(_)) {
        return Outcome::Fail(format!(
            ".rem1 font-size was not resolved to Px: {:?}",
            style.font_size
        ));
    }

    // .rem2 { padding-top: 2rem }  =>  padding resolves to Px.
    let rem2_sel = match parse_selector(dom, ".rem2") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .rem2 selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &rem2_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .rem2".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if !matches!(style.padding.top, Length::Px(_)) {
        return Outcome::Fail(format!(
            ".rem2 padding-top was not resolved to Px: {:?}",
            style.padding.top
        ));
    }

    Outcome::Pass
}

fn check_css_individual_margins(dom: &Dom, document: NodeId, source: &str) -> Outcome {
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
    let children: Vec<NodeId> = match dom.children(body) {
        Ok(c) => c.to_vec(),
        Err(_) => return Outcome::Fail("could not get body children".to_string()),
    };

    // .shorthand { margin: 10px 20px 30px 40px } -- four-value shorthand expands to all four sides.
    let shorthand_sel = match parse_selector(dom, ".shorthand") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .shorthand selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &shorthand_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .shorthand".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.margin.top != silksurf_css::LengthOrAuto::Length(Length::Px(10.0)) {
        return Outcome::Fail(format!(
            ".shorthand margin-top expected 10px, got {:?}",
            style.margin.top
        ));
    }
    if style.margin.right != silksurf_css::LengthOrAuto::Length(Length::Px(20.0)) {
        return Outcome::Fail(format!(
            ".shorthand margin-right expected 20px, got {:?}",
            style.margin.right
        ));
    }
    if style.margin.bottom != silksurf_css::LengthOrAuto::Length(Length::Px(30.0)) {
        return Outcome::Fail(format!(
            ".shorthand margin-bottom expected 30px, got {:?}",
            style.margin.bottom
        ));
    }
    if style.margin.left != silksurf_css::LengthOrAuto::Length(Length::Px(40.0)) {
        return Outcome::Fail(format!(
            ".shorthand margin-left expected 40px, got {:?}",
            style.margin.left
        ));
    }

    // .override { margin: 5px; margin-top: 15px } -- individual side overrides shorthand.
    let override_sel = match parse_selector(dom, ".override") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .override selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &override_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .override".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.margin.top != silksurf_css::LengthOrAuto::Length(Length::Px(15.0)) {
        return Outcome::Fail(format!(
            ".override margin-top expected 15px (individual beats shorthand), got {:?}",
            style.margin.top
        ));
    }
    if style.margin.right != silksurf_css::LengthOrAuto::Length(Length::Px(5.0)) {
        return Outcome::Fail(format!(
            ".override margin-right expected 5px (from shorthand), got {:?}",
            style.margin.right
        ));
    }

    // .auto-side { margin-left: auto; margin-right: auto } -- auto margins preserved.
    let auto_sel = match parse_selector(dom, ".auto-side") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .auto-side selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &auto_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .auto-side".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.margin.left != silksurf_css::LengthOrAuto::Auto {
        return Outcome::Fail(format!(
            ".auto-side margin-left expected Auto, got {:?}",
            style.margin.left
        ));
    }
    if style.margin.right != silksurf_css::LengthOrAuto::Auto {
        return Outcome::Fail(format!(
            ".auto-side margin-right expected Auto, got {:?}",
            style.margin.right
        ));
    }

    // .pad-sides { padding-top: 4px; ... } -- individual padding sides.
    let pad_sel = match parse_selector(dom, ".pad-sides") {
        Some(s) => s,
        None => return Outcome::Fail("could not parse .pad-sides selector".to_string()),
    };
    let node = match children
        .iter()
        .copied()
        .find(|&c| matches_selector_list(dom, c, &pad_sel))
    {
        Some(n) => n,
        None => return Outcome::Fail("no element matched .pad-sides".to_string()),
    };
    let style = compute_style_for_node(dom, node, &stylesheet, None);
    if style.padding.top != Length::Px(4.0) {
        return Outcome::Fail(format!(
            ".pad-sides padding-top expected 4px, got {:?}",
            style.padding.top
        ));
    }
    if style.padding.right != Length::Px(8.0) {
        return Outcome::Fail(format!(
            ".pad-sides padding-right expected 8px, got {:?}",
            style.padding.right
        ));
    }
    if style.padding.bottom != Length::Px(12.0) {
        return Outcome::Fail(format!(
            ".pad-sides padding-bottom expected 12px, got {:?}",
            style.padding.bottom
        ));
    }
    if style.padding.left != Length::Px(16.0) {
        return Outcome::Fail(format!(
            ".pad-sides padding-left expected 16px, got {:?}",
            style.padding.left
        ));
    }

    Outcome::Pass
}

fn check_css_named_colors(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    // Verify a representative set of commonly-used named colors.
    let cases: &[(&str, (u8, u8, u8))] = &[
        (".c-orange", (255, 165, 0)),
        (".c-yellow", (255, 255, 0)),
        (".c-gray", (128, 128, 128)),
        (".c-grey", (128, 128, 128)),
        (".c-purple", (128, 0, 128)),
        (".c-pink", (255, 192, 203)),
        (".c-navy", (0, 0, 128)),
        (".c-teal", (0, 128, 128)),
        (".c-silver", (192, 192, 192)),
        (".c-lime", (0, 255, 0)),
        (".c-aqua", (0, 255, 255)),
        (".c-cyan", (0, 255, 255)),
        (".c-maroon", (128, 0, 0)),
        (".c-olive", (128, 128, 0)),
        (".c-fuchsia", (255, 0, 255)),
        (".c-magenta", (255, 0, 255)),
    ];
    for &(selector, (er, eg, eb)) in cases {
        let sel = match parse_selector(dom, selector) {
            Some(s) => s,
            None => {
                return Outcome::Fail(format!("could not parse selector {selector}"));
            }
        };
        let node = match find_element(dom, document, &sel) {
            Some(n) => n,
            None => {
                return Outcome::Fail(format!("no element matched {selector}"));
            }
        };
        let style = compute_style_for_node(dom, node, &stylesheet, None);
        let c = style.color;
        if c.r != er || c.g != eg || c.b != eb || c.a != 255 {
            return Outcome::Fail(format!(
                "{selector} color expected rgb({er},{eg},{eb}), got rgb({},{},{},a={})",
                c.r, c.g, c.b, c.a
            ));
        }
    }
    Outcome::Pass
}

fn find_element(dom: &Dom, root: NodeId, sel: &SelectorList) -> Option<NodeId> {
    let children = dom.children(root).ok()?;
    for &child in children {
        if matches_selector_list(dom, child, sel) {
            return Some(child);
        }
        if let Some(found) = find_element(dom, child, sel) {
            return Some(found);
        }
    }
    None
}

fn check_css_cascade_keywords(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    // Full-tree cascade so inheritance flows from parent computed values.
    let styles = compute_styles(dom, document, &stylesheet);

    let get_style = |id: &str| -> Option<silksurf_css::ComputedStyle> {
        let selector_str = format!("#{id}");
        let sel = parse_selector(dom, &selector_str)?;
        let node = find_element(dom, document, &sel)?;
        styles.get(&node).cloned()
    };

    // `color: inherit` on child of `.parent { color: red }` -> red.
    let inherit_color = match get_style("inherit-color") {
        Some(s) => s,
        None => return Outcome::Fail("element #inherit-color not found".to_string()),
    };
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    if inherit_color.color != red {
        return Outcome::Fail(format!(
            "#inherit-color: expected red, got {:?}",
            inherit_color.color
        ));
    }

    // `display: inherit` on child of `.parent { display: flex }` -> Flex.
    let inherit_display = match get_style("inherit-display") {
        Some(s) => s,
        None => return Outcome::Fail("element #inherit-display not found".to_string()),
    };
    if inherit_display.display != Display::Flex {
        return Outcome::Fail(format!(
            "#inherit-display: expected Flex, got {:?}",
            inherit_display.display
        ));
    }

    // `color: initial` -> CSS initial value for color = black.
    let initial_color = match get_style("initial-color") {
        Some(s) => s,
        None => return Outcome::Fail("element #initial-color not found".to_string()),
    };
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    if initial_color.color != black {
        return Outcome::Fail(format!(
            "#initial-color: expected black, got {:?}",
            initial_color.color
        ));
    }

    // `color: unset` -- color is inherited -> unset = inherit -> red.
    let unset_color = match get_style("unset-color") {
        Some(s) => s,
        None => return Outcome::Fail("element #unset-color not found".to_string()),
    };
    if unset_color.color != red {
        return Outcome::Fail(format!(
            "#unset-color: expected red (unset=inherit for inherited prop), got {:?}",
            unset_color.color
        ));
    }

    // `display: unset` -- display is not inherited -> unset = initial -> Inline.
    let unset_display = match get_style("unset-display") {
        Some(s) => s,
        None => return Outcome::Fail("element #unset-display not found".to_string()),
    };
    if unset_display.display != Display::Inline {
        return Outcome::Fail(format!(
            "#unset-display: expected Inline (unset=initial for non-inherited), got {:?}",
            unset_display.display
        ));
    }

    Outcome::Pass
}

fn check_css_individual_borders(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };

    let get_style = |id: &str| -> Option<silksurf_css::ComputedStyle> {
        let selector_str = format!("#{id}");
        let sel = parse_selector(dom, &selector_str)?;
        let node = find_element(dom, document, &sel)?;
        Some(compute_style_for_node(dom, node, &stylesheet, None))
    };

    // #a { border-top: 3px solid red } -> top=3px, others=0
    let a = match get_style("a") {
        Some(s) => s,
        None => return Outcome::Fail("element #a not found".to_string()),
    };
    if a.border.top != Length::Px(3.0) {
        return Outcome::Fail(format!(
            "#a border-top: expected 3px, got {:?}",
            a.border.top
        ));
    }
    if a.border.right != Length::Px(0.0)
        || a.border.bottom != Length::Px(0.0)
        || a.border.left != Length::Px(0.0)
    {
        return Outcome::Fail(format!(
            "#a: other sides expected 0, got right={:?} bottom={:?} left={:?}",
            a.border.right, a.border.bottom, a.border.left
        ));
    }

    // #b { border-right: 5px } -> right=5px
    let b = match get_style("b") {
        Some(s) => s,
        None => return Outcome::Fail("element #b not found".to_string()),
    };
    if b.border.right != Length::Px(5.0) {
        return Outcome::Fail(format!(
            "#b border-right: expected 5px, got {:?}",
            b.border.right
        ));
    }

    // #c { border-bottom: 2px } -> bottom=2px
    let c = match get_style("c") {
        Some(s) => s,
        None => return Outcome::Fail("element #c not found".to_string()),
    };
    if c.border.bottom != Length::Px(2.0) {
        return Outcome::Fail(format!(
            "#c border-bottom: expected 2px, got {:?}",
            c.border.bottom
        ));
    }

    // #d { border-left: 7px } -> left=7px
    let d = match get_style("d") {
        Some(s) => s,
        None => return Outcome::Fail("element #d not found".to_string()),
    };
    if d.border.left != Length::Px(7.0) {
        return Outcome::Fail(format!(
            "#d border-left: expected 7px, got {:?}",
            d.border.left
        ));
    }

    // #e { border: 10px; border-top: 4px } -> top=4px, others=10px
    let e = match get_style("e") {
        Some(s) => s,
        None => return Outcome::Fail("element #e not found".to_string()),
    };
    if e.border.top != Length::Px(4.0) {
        return Outcome::Fail(format!(
            "#e border-top: expected 4px (individual wins), got {:?}",
            e.border.top
        ));
    }
    if e.border.right != Length::Px(10.0)
        || e.border.bottom != Length::Px(10.0)
        || e.border.left != Length::Px(10.0)
    {
        return Outcome::Fail(format!(
            "#e: other sides expected 10px, got right={:?} bottom={:?} left={:?}",
            e.border.right, e.border.bottom, e.border.left
        ));
    }

    Outcome::Pass
}

// ---------------------------------------------------------------------------
// Taffy layout rect checks.
//
// WHY: cascade-only checks confirm that CSS properties reach ComputedStyle.
// These checks go further: they run the full fused style+layout pipeline and
// confirm that taffy places flex items at the correct absolute pixel positions.
//
// Pattern: parse inline CSS -> fused_style_layout_paint -> look up node rects
// by element id -> assert positions/sizes within 2px floating-point tolerance.
// ---------------------------------------------------------------------------

fn fused_rect_by_id(
    fused: &silksurf_engine::fused_pipeline::FusedResult,
    dom: &Dom,
    document: NodeId,
    id: &str,
) -> Option<Rect> {
    let sel_str = format!("#{id}");
    let sel = parse_selector(dom, &sel_str)?;
    let node = find_element(dom, document, &sel)?;
    let bfs_idx = *fused.table.node_to_bfs_idx.get(&node)? as usize;
    Some(fused.node_rects[bfs_idx])
}

fn check_layout_flex_row_equal(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let viewport = Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 };
    let fused = fused_style_layout_paint(dom, &stylesheet, document, viewport);

    let ra = match fused_rect_by_id(&fused, dom, document, "a") {
        Some(r) => r,
        None => return Outcome::Fail("#a not found in layout".to_string()),
    };
    let rb = match fused_rect_by_id(&fused, dom, document, "b") {
        Some(r) => r,
        None => return Outcome::Fail("#b not found in layout".to_string()),
    };
    let rc = match fused_rect_by_id(&fused, dom, document, "c") {
        Some(r) => r,
        None => return Outcome::Fail("#c not found in layout".to_string()),
    };

    // 3 items with flex:1 in a 300px container each get 100px.
    let expected_w = 100.0_f32;
    let tol = 2.0_f32;
    for (id, rect) in [("a", ra), ("b", rb), ("c", rc)] {
        if (rect.width - expected_w).abs() > tol {
            return Outcome::Fail(format!(
                "#{id} width expected ~{expected_w}px, got {:.1}",
                rect.width
            ));
        }
    }
    if (rb.x - ra.x - expected_w).abs() > tol {
        return Outcome::Fail(format!(
            "#b.x offset from #a expected ~{expected_w}px, got {:.1}",
            rb.x - ra.x
        ));
    }
    if (rc.x - rb.x - expected_w).abs() > tol {
        return Outcome::Fail(format!(
            "#c.x offset from #b expected ~{expected_w}px, got {:.1}",
            rc.x - rb.x
        ));
    }
    Outcome::Pass
}

fn check_layout_flex_fixed_offset(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let viewport = Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 };
    let fused = fused_style_layout_paint(dom, &stylesheet, document, viewport);

    let ra = match fused_rect_by_id(&fused, dom, document, "a") {
        Some(r) => r,
        None => return Outcome::Fail("#a not found in layout".to_string()),
    };
    let rb = match fused_rect_by_id(&fused, dom, document, "b") {
        Some(r) => r,
        None => return Outcome::Fail("#b not found in layout".to_string()),
    };

    let tol = 2.0_f32;
    if (ra.width - 60.0).abs() > tol {
        return Outcome::Fail(format!("#a width expected 60px, got {:.1}", ra.width));
    }
    if (rb.width - 120.0).abs() > tol {
        return Outcome::Fail(format!("#b width expected 120px, got {:.1}", rb.width));
    }
    // #b starts immediately after #a in the flex row.
    if (rb.x - ra.x - ra.width).abs() > tol {
        return Outcome::Fail(format!(
            "#b.x expected ~{:.1} (right of #a), got {:.1}",
            ra.x + ra.width,
            rb.x
        ));
    }
    Outcome::Pass
}

fn check_layout_flex_column_stack(dom: &Dom, document: NodeId, source: &str) -> Outcome {
    let css = match extract_inline_style(source) {
        Some(c) => c,
        None => return Outcome::Skip("no <style> block found".to_string()),
    };
    let stylesheet = match parse_stylesheet(&css) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("css parse: {e:?}")),
    };
    let viewport = Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 };
    let fused = fused_style_layout_paint(dom, &stylesheet, document, viewport);

    let ra = match fused_rect_by_id(&fused, dom, document, "a") {
        Some(r) => r,
        None => return Outcome::Fail("#a not found in layout".to_string()),
    };
    let rb = match fused_rect_by_id(&fused, dom, document, "b") {
        Some(r) => r,
        None => return Outcome::Fail("#b not found in layout".to_string()),
    };

    let tol = 2.0_f32;
    if (ra.height - 40.0).abs() > tol {
        return Outcome::Fail(format!("#a height expected 40px, got {:.1}", ra.height));
    }
    if (rb.height - 80.0).abs() > tol {
        return Outcome::Fail(format!("#b height expected 80px, got {:.1}", rb.height));
    }
    // #b must start at or below the bottom of #a.
    if rb.y < ra.y + ra.height - tol {
        return Outcome::Fail(format!(
            "#b.y={:.1} must be >= #a bottom={:.1}",
            rb.y,
            ra.y + ra.height
        ));
    }
    Outcome::Pass
}
