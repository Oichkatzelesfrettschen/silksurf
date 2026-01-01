use crate::matching::{matches_selector, selector_specificity, Specificity};
use crate::selector::{Selector, SelectorIdent, SelectorModifier, TypeSelector};
use crate::{CssToken, Declaration, Rule, Stylesheet};
use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind, TagName};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Inline,
    Block,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
}

impl Length {
    pub fn zero() -> Self {
        Length::Px(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn black() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    pub fn transparent() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
}

impl Edges {
    pub fn all(value: Length) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Color,
    pub background_color: Color,
    pub font_size: Length,
    pub line_height: Length,
    pub font_family: Vec<String>,
    pub margin: Edges,
    pub padding: Edges,
    pub border: Edges,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            color: Color::black(),
            background_color: Color::transparent(),
            font_size: Length::Px(16.0),
            line_height: Length::Px(16.0),
            font_family: vec!["sans-serif".to_string()],
            margin: Edges::all(Length::zero()),
            padding: Edges::all(Length::zero()),
            border: Edges::all(Length::zero()),
        }
    }
}

#[derive(Clone)]
struct ResolvedProperty<T> {
    value: T,
    important: bool,
    specificity: Specificity,
    order: usize,
}

impl<T: Clone> ResolvedProperty<T> {
    fn should_override(&self, candidate: &ResolvedProperty<T>) -> bool {
        if self.important != candidate.important {
            return candidate.important;
        }
        if self.specificity != candidate.specificity {
            return candidate.specificity > self.specificity;
        }
        candidate.order > self.order
    }
}

#[derive(Default)]
struct CascadedStyle {
    display: Option<ResolvedProperty<Display>>,
    color: Option<ResolvedProperty<Color>>,
    background_color: Option<ResolvedProperty<Color>>,
    font_size: Option<ResolvedProperty<Length>>,
    line_height: Option<ResolvedProperty<Length>>,
    font_family: Option<ResolvedProperty<Vec<String>>>,
    margin: Option<ResolvedProperty<Edges>>,
    padding: Option<ResolvedProperty<Edges>>,
    border: Option<ResolvedProperty<Edges>>,
}

impl CascadedStyle {
    fn resolve(self, parent: Option<&ComputedStyle>) -> ComputedStyle {
        let fallback = ComputedStyle::default();
        let resolved_font_size = self
            .font_size
            .map(|entry| entry.value)
            .or_else(|| parent.map(|style| style.font_size))
            .unwrap_or(fallback.font_size);
        ComputedStyle {
            display: self
                .display
                .map(|entry| entry.value)
                .unwrap_or(fallback.display),
            color: self
                .color
                .map(|entry| entry.value)
                .or_else(|| parent.map(|style| style.color))
                .unwrap_or(fallback.color),
            background_color: self
                .background_color
                .map(|entry| entry.value)
                .unwrap_or(fallback.background_color),
            font_size: resolved_font_size,
            line_height: self
                .line_height
                .map(|entry| entry.value)
                .or_else(|| parent.map(|style| style.line_height))
                .unwrap_or(resolved_font_size),
            font_family: self
                .font_family
                .map(|entry| entry.value)
                .or_else(|| parent.map(|style| style.font_family.clone()))
                .unwrap_or(fallback.font_family),
            margin: self
                .margin
                .map(|entry| entry.value)
                .unwrap_or(fallback.margin),
            padding: self
                .padding
                .map(|entry| entry.value)
                .unwrap_or(fallback.padding),
            border: self
                .border
                .map(|entry| entry.value)
                .unwrap_or(fallback.border),
        }
    }
}

fn apply_property<T: Clone>(
    slot: &mut Option<ResolvedProperty<T>>,
    value: T,
    important: bool,
    specificity: Specificity,
    order: usize,
) {
    let candidate = ResolvedProperty {
        value,
        important,
        specificity,
        order,
    };
    match slot {
        Some(existing) => {
            if existing.should_override(&candidate) {
                *slot = Some(candidate);
            }
        }
        None => {
            *slot = Some(candidate);
        }
    }
}

#[derive(Clone)]
struct IndexedSelector {
    rule_index: usize,
    selector_index: usize,
    specificity: Specificity,
}

struct StyleIndex {
    tag_rules: HashMap<TagName, Vec<IndexedSelector>>,
    id_rules: HashMap<SelectorIdent, Vec<IndexedSelector>>,
    class_rules: HashMap<SelectorIdent, Vec<IndexedSelector>>,
    universal_rules: Vec<IndexedSelector>,
}

impl StyleIndex {
    fn new(stylesheet: &Stylesheet) -> Self {
        let mut index = StyleIndex {
            tag_rules: HashMap::new(),
            id_rules: HashMap::new(),
            class_rules: HashMap::new(),
            universal_rules: Vec::new(),
        };
        for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
            let Rule::Style(rule) = rule else {
                continue;
            };
            for (selector_index, selector) in rule.selectors.selectors.iter().enumerate() {
                let entry = IndexedSelector {
                    rule_index,
                    selector_index,
                    specificity: selector_specificity(selector),
                };
                match selector_key(selector) {
                    SelectorKey::Tag(tag) => {
                        index.tag_rules.entry(tag).or_default().push(entry);
                    }
                    SelectorKey::Id(id) => {
                        index.id_rules.entry(id).or_default().push(entry);
                    }
                    SelectorKey::Class(class) => {
                        index.class_rules.entry(class).or_default().push(entry);
                    }
                    SelectorKey::Universal => index.universal_rules.push(entry),
                }
            }
        }
        index
    }
}

enum SelectorKey {
    Tag(TagName),
    Id(SelectorIdent),
    Class(SelectorIdent),
    Universal,
}

fn selector_key(selector: &Selector) -> SelectorKey {
    let Some(step) = selector.steps.last() else {
        return SelectorKey::Universal;
    };
    let compound = &step.compound;
    let mut id_key = None;
    let mut class_key = None;
    for modifier in &compound.modifiers {
        match modifier {
            SelectorModifier::Id(name) => {
                id_key = Some(name.clone());
                break;
            }
            SelectorModifier::Class(name) if class_key.is_none() => {
                class_key = Some(name.clone());
            }
            _ => {}
        }
    }
    if let Some(id) = id_key {
        return SelectorKey::Id(id);
    }
    if let Some(class) = class_key {
        return SelectorKey::Class(class);
    }
    match compound.type_selector.as_ref() {
        Some(TypeSelector::Tag(tag)) => SelectorKey::Tag(tag.clone()),
        _ => SelectorKey::Universal,
    }
}

fn node_tag(dom: &Dom, node: NodeId) -> Option<TagName> {
    let Ok(node) = dom.node(node) else {
        return None;
    };
    match node.kind() {
        NodeKind::Element { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn node_id_class_keys(dom: &Dom, node: NodeId) -> (Option<SelectorIdent>, Vec<SelectorIdent>) {
    let attrs = match dom.attributes(node) {
        Ok(attrs) => attrs,
        Err(_) => return (None, Vec::new()),
    };
    let mut id_key = None;
    let mut class_keys = Vec::new();
    for attr in attrs {
        match attr.name {
            AttributeName::Id => {
                if let Some(atom) = attr.value_atom {
                    id_key = Some(SelectorIdent::new_with_atom(dom.resolve(atom), atom));
                } else {
                    id_key = Some(SelectorIdent::from(attr.value.clone()));
                }
            }
            AttributeName::Class => {
                if !attr.value_atoms.is_empty() {
                    for atom in &attr.value_atoms {
                        class_keys.push(SelectorIdent::new_with_atom(dom.resolve(*atom), *atom));
                    }
                } else {
                    for part in attr.value.as_str().split_whitespace() {
                        class_keys.push(SelectorIdent::new(part));
                    }
                }
            }
            _ => {}
        }
    }
    (id_key, class_keys)
}

pub fn compute_styles(
    dom: &Dom,
    root: NodeId,
    stylesheet: &Stylesheet,
) -> HashMap<NodeId, ComputedStyle> {
    let index = StyleIndex::new(stylesheet);
    let mut styles = HashMap::new();
    compute_styles_recursive(dom, root, stylesheet, &index, None, &mut styles);
    styles
}

// Used by silksurf-engine; not referenced internally in this crate yet.
#[allow(dead_code)]
pub struct StyleCache {
    generation: u64,
    styles: Arc<HashMap<NodeId, ComputedStyle>>,
}

#[allow(dead_code)]
impl StyleCache {
    pub fn new() -> Self {
        Self {
            generation: 0,
            styles: Arc::new(HashMap::new()),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn styles(&self) -> &HashMap<NodeId, ComputedStyle> {
        self.styles.as_ref()
    }

    pub fn styles_arc(&self) -> Arc<HashMap<NodeId, ComputedStyle>> {
        Arc::clone(&self.styles)
    }

    pub fn compute(
        &mut self,
        dom: &Dom,
        root: NodeId,
        stylesheet: &Stylesheet,
    ) -> Arc<HashMap<NodeId, ComputedStyle>> {
        self.generation = self.generation.wrapping_add(1);
        self.styles = Arc::new(compute_styles(dom, root, stylesheet));
        Arc::clone(&self.styles)
    }

    pub fn compute_incremental(
        &mut self,
        dom: &Dom,
        root: NodeId,
        stylesheet: &Stylesheet,
        dirty_nodes: &[NodeId],
    ) -> Arc<HashMap<NodeId, ComputedStyle>> {
        if dirty_nodes.is_empty() {
            if self.styles.is_empty() {
                return self.compute(dom, root, stylesheet);
            }
            return Arc::clone(&self.styles);
        }

        if self.styles.is_empty() {
            return self.compute(dom, root, stylesheet);
        }

        let mut filtered = Vec::new();
        for node in dirty_nodes {
            if *node == root {
                filtered.push(*node);
                continue;
            }
            if dom.element_name(*node).ok().flatten().is_some() {
                filtered.push(*node);
            }
        }

        if filtered.is_empty() {
            return Arc::clone(&self.styles);
        }

        let mut needs_full = false;
        for node in &filtered {
            if *node == root {
                needs_full = true;
                break;
            }
            let parent = dom.parent(*node).ok().flatten();
            match parent {
                Some(parent) if self.styles.contains_key(&parent) => {}
                _ => {
                    needs_full = true;
                    break;
                }
            }
        }

        if needs_full {
            return self.compute(dom, root, stylesheet);
        }

        let index = StyleIndex::new(stylesheet);
        self.generation = self.generation.wrapping_add(1);
        let styles = Arc::make_mut(&mut self.styles);
        let mut seen = HashSet::new();
        for node in &filtered {
            if !seen.insert(*node) {
                continue;
            }
            let parent_style = dom
                .parent(*node)
                .ok()
                .flatten()
                .and_then(|parent| styles.get(&parent).cloned());
            compute_styles_recursive(
                dom,
                *node,
                stylesheet,
                &index,
                parent_style.as_ref(),
                styles,
            );
        }

        Arc::clone(&self.styles)
    }
}

pub fn compute_style_for_node(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    parent: Option<&ComputedStyle>,
) -> ComputedStyle {
    let index = StyleIndex::new(stylesheet);
    compute_style_for_node_with_index(dom, node, stylesheet, &index, parent)
}

fn compute_style_for_node_with_index(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    parent: Option<&ComputedStyle>,
) -> ComputedStyle {
    if dom.element_name(node).ok().flatten().is_none() {
        return parent.cloned().unwrap_or_default();
    }
    cascade_for_node(dom, node, stylesheet, index).resolve(parent)
}

fn compute_styles_recursive(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    parent: Option<&ComputedStyle>,
    styles: &mut HashMap<NodeId, ComputedStyle>,
) {
    let style = compute_style_for_node_with_index(dom, node, stylesheet, index, parent);
    styles.insert(node, style.clone());
    if let Ok(children) = dom.children(node) {
        for child in children {
            compute_styles_recursive(dom, *child, stylesheet, index, Some(&style), styles);
        }
    }
}

fn cascade_for_node(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
) -> CascadedStyle {
    let mut cascaded = CascadedStyle::default();
    let mut order = 0usize;
    let mut matched_by_rule: Vec<Option<Specificity>> =
        vec![None; stylesheet.rules.len()];
    let mut candidates: Vec<IndexedSelector> = Vec::new();
    if let Some(tag) = node_tag(dom, node) {
        if let Some(entries) = index.tag_rules.get(&tag) {
            candidates.extend(entries.iter().cloned());
        }
    }
    let (id_key, class_keys) = node_id_class_keys(dom, node);
    if let Some(id_key) = id_key {
        if let Some(entries) = index.id_rules.get(&id_key) {
            candidates.extend(entries.iter().cloned());
        }
    }
    for class_key in class_keys {
        if let Some(entries) = index.class_rules.get(&class_key) {
            candidates.extend(entries.iter().cloned());
        }
    }
    candidates.extend(index.universal_rules.iter().cloned());

    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert((candidate.rule_index, candidate.selector_index)) {
            continue;
        }
        let Some(rule) = stylesheet.rules.get(candidate.rule_index) else {
            continue;
        };
        let Rule::Style(rule) = rule else {
            continue;
        };
        let Some(selector) = rule.selectors.selectors.get(candidate.selector_index) else {
            continue;
        };
        if matches_selector(dom, node, selector) {
            if let Some(slot) = matched_by_rule.get_mut(candidate.rule_index) {
                match slot {
                    Some(existing) => {
                        if candidate.specificity > *existing {
                            *existing = candidate.specificity;
                        }
                    }
                    None => {
                        *slot = Some(candidate.specificity);
                    }
                }
            }
        }
    }

    for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
        let Some(specificity) = matched_by_rule
            .get(rule_index)
            .and_then(|spec| *spec)
        else {
            continue;
        };
        let Rule::Style(rule) = rule else {
            continue;
        };
        for declaration in &rule.declarations {
            order += 1;
            apply_declaration(&mut cascaded, declaration, specificity, order);
        }
    }
    cascaded
}
fn apply_declaration(
    cascaded: &mut CascadedStyle,
    declaration: &Declaration,
    specificity: Specificity,
    order: usize,
) {
    let name = declaration.name.to_ascii_lowercase();
    match name.as_str() {
        "display" => {
            if let Some(value) = parse_display(&declaration.value) {
                apply_property(
                    &mut cascaded.display,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "color" => {
            if let Some(value) = parse_color(&declaration.value) {
                apply_property(
                    &mut cascaded.color,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "background-color" => {
            if let Some(value) = parse_color(&declaration.value) {
                apply_property(
                    &mut cascaded.background_color,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "font-size" => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.font_size,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "line-height" => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.line_height,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "font-family" => {
            if let Some(value) = parse_font_family(&declaration.value) {
                apply_property(
                    &mut cascaded.font_family,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "margin" => {
            if let Some(value) = parse_edges(&declaration.value) {
                apply_property(
                    &mut cascaded.margin,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "padding" => {
            if let Some(value) = parse_edges(&declaration.value) {
                apply_property(
                    &mut cascaded.padding,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "border" | "border-width" => {
            if let Some(value) = parse_edges(&declaration.value) {
                apply_property(
                    &mut cascaded.border,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        _ => {}
    }
}

fn parse_display(tokens: &[CssToken]) -> Option<Display> {
    let ident = tokens.iter().find_map(|token| match token {
        CssToken::Ident(value) => Some(value.as_str()),
        _ => None,
    })?;
    match ident.to_ascii_lowercase().as_str() {
        "block" => Some(Display::Block),
        "inline" => Some(Display::Inline),
        "none" => Some(Display::None),
        _ => None,
    }
}

fn parse_length(tokens: &[CssToken]) -> Option<Length> {
    tokens.iter().find_map(parse_length_token)
}

fn parse_length_token(token: &CssToken) -> Option<Length> {
    match token {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok().map(Length::Px)
        }
        CssToken::Number(value) if value == "0" => Some(Length::zero()),
        _ => None,
    }
}
fn parse_length_list(tokens: &[CssToken]) -> Vec<Length> {
    let mut values = Vec::new();
    for token in tokens {
        if let Some(length) = parse_length_token(token) {
            values.push(length);
        }
    }
    values
}

fn parse_edges(tokens: &[CssToken]) -> Option<Edges> {
    let values = parse_length_list(tokens);
    match values.len() {
        1 => Some(Edges::all(values[0])),
        2 => Some(Edges {
            top: values[0],
            right: values[1],
            bottom: values[0],
            left: values[1],
        }),
        3 => Some(Edges {
            top: values[0],
            right: values[1],
            bottom: values[2],
            left: values[1],
        }),
        4 => Some(Edges {
            top: values[0],
            right: values[1],
            bottom: values[2],
            left: values[3],
        }),
        _ => None,
    }
}

fn parse_font_family(tokens: &[CssToken]) -> Option<Vec<String>> {
    let mut families = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        match token {
            CssToken::Ident(value) | CssToken::String(value) => {
                current.push(value.clone());
            }
            CssToken::Comma => {
                if !current.is_empty() {
                    families.push(current.join(" "));
                    current.clear();
                }
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        families.push(current.join(" "));
    }
    if families.is_empty() {
        None
    } else {
        Some(families)
    }
}
fn parse_color(tokens: &[CssToken]) -> Option<Color> {
    let mut iter = tokens.iter().filter(|token| !matches!(token, CssToken::Whitespace));
    match iter.next()? {
        CssToken::Ident(value) => parse_named_color(value),
        CssToken::Hash(value) => parse_hex_color(value),
        CssToken::Function(name) if name.eq_ignore_ascii_case("rgb") => parse_rgb_function(iter),
        _ => None,
    }
}

fn parse_named_color(value: &str) -> Option<Color> {
    match value.to_ascii_lowercase().as_str() {
        "black" => Some(Color::black()),
        "white" => Some(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }),
        "red" => Some(Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }),
        "green" => Some(Color {
            r: 0,
            g: 128,
            b: 0,
            a: 255,
        }),
        "blue" => Some(Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        }),
        "transparent" => Some(Color::transparent()),
        _ => None,
    }
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let value = value.trim();
    match value.len() {
        3 => {
            let r = hex_to_u8(&value[0..1])?;
            let g = hex_to_u8(&value[1..2])?;
            let b = hex_to_u8(&value[2..3])?;
            Some(Color {
                r: r * 17,
                g: g * 17,
                b: b * 17,
                a: 255,
            })
        }
        6 => {
            let r = hex_to_u8(&value[0..2])?;
            let g = hex_to_u8(&value[2..4])?;
            let b = hex_to_u8(&value[4..6])?;
            Some(Color { r, g, b, a: 255 })
        }
        _ => None,
    }
}

fn parse_rgb_function<'a, I>(iter: I) -> Option<Color>
where
    I: Iterator<Item = &'a CssToken>,
{
    let mut values = Vec::new();
    for token in iter {
        match token {
            CssToken::Number(_) | CssToken::Percentage(_) => {
                if let Some(value) = parse_rgb_component(token) {
                    values.push(value);
                }
            }
            CssToken::Comma | CssToken::Whitespace => {}
            CssToken::ParenClose => break,
            _ => {}
        }
    }
    if values.len() == 3 {
        Some(Color {
            r: values[0],
            g: values[1],
            b: values[2],
            a: 255,
        })
    } else {
        None
    }
}

fn parse_rgb_component(token: &CssToken) -> Option<u8> {
    match token {
        CssToken::Number(value) => value
            .parse::<f32>()
            .ok()
            .map(|number| number.clamp(0.0, 255.0) as u8),
        CssToken::Percentage(value) => value
            .parse::<f32>()
            .ok()
            .map(|percent| ((percent.clamp(0.0, 100.0) / 100.0) * 255.0) as u8),
        _ => None,
    }
}

fn hex_to_u8(value: &str) -> Option<u8> {
    u8::from_str_radix(value, 16).ok()
}
