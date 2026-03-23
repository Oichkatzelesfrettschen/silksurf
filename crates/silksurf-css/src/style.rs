use crate::matching::{Specificity, matches_selector, selector_specificity};
use crate::selector::{Selector, SelectorIdent, SelectorModifier, TypeSelector};
use crate::{CssToken, Declaration, Rule, Stylesheet};
use rustc_hash::{FxHashMap, FxHashSet};
use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind, TagName};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Inline,
    Block,
    Flex,
    InlineFlex,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexWrap {
    #[default]
    Nowrap,
    Wrap,
    WrapReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignItems {
    #[default]
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignSelf {
    Auto,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    Baseline,
}

impl Default for AlignSelf {
    fn default() -> Self {
        AlignSelf::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlexItemStyle {
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: FlexBasis,
    pub align_self: AlignSelf,
    pub order: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexBasis {
    Auto,
    Length(Length),
}

impl Default for FlexBasis {
    fn default() -> Self {
        FlexBasis::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlexContainerStyle {
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub gap: f32,
    pub row_gap: f32,
    pub column_gap: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
}

impl Length {
    pub fn zero() -> Self {
        Length::Px(0.0)
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Length::Px(v) | Length::Percent(v) => *v == 0.0,
        }
    }
}

/// Optional length value (for top/right/bottom/left offsets).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LengthOrAuto {
    #[default]
    Auto,
    Length(Length),
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
    // Flex container properties
    pub flex_container: FlexContainerStyle,
    // Flex item properties
    pub flex_item: FlexItemStyle,
    // Positioning
    pub position: Position,
    pub top: LengthOrAuto,
    pub right: LengthOrAuto,
    pub bottom: LengthOrAuto,
    pub left: LengthOrAuto,
    pub z_index: i32,
    // Overflow
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    // Visual
    pub opacity: f32,
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
            flex_container: FlexContainerStyle::default(),
            flex_item: FlexItemStyle::default(),
            position: Position::default(),
            top: LengthOrAuto::Auto,
            right: LengthOrAuto::Auto,
            bottom: LengthOrAuto::Auto,
            left: LengthOrAuto::Auto,
            z_index: 0,
            overflow_x: Overflow::default(),
            overflow_y: Overflow::default(),
            opacity: 1.0,
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
    // Flex container
    flex_direction: Option<ResolvedProperty<FlexDirection>>,
    flex_wrap: Option<ResolvedProperty<FlexWrap>>,
    justify_content: Option<ResolvedProperty<JustifyContent>>,
    align_items: Option<ResolvedProperty<AlignItems>>,
    gap: Option<ResolvedProperty<f32>>,
    row_gap: Option<ResolvedProperty<f32>>,
    column_gap: Option<ResolvedProperty<f32>>,
    // Flex item
    flex_grow: Option<ResolvedProperty<f32>>,
    flex_shrink: Option<ResolvedProperty<f32>>,
    flex_basis: Option<ResolvedProperty<FlexBasis>>,
    align_self: Option<ResolvedProperty<AlignSelf>>,
    order: Option<ResolvedProperty<i32>>,
    // Positioning
    position: Option<ResolvedProperty<Position>>,
    top: Option<ResolvedProperty<LengthOrAuto>>,
    right_offset: Option<ResolvedProperty<LengthOrAuto>>,
    bottom: Option<ResolvedProperty<LengthOrAuto>>,
    left_offset: Option<ResolvedProperty<LengthOrAuto>>,
    z_index: Option<ResolvedProperty<i32>>,
    // Overflow
    overflow_x: Option<ResolvedProperty<Overflow>>,
    overflow_y: Option<ResolvedProperty<Overflow>>,
    // Visual
    opacity: Option<ResolvedProperty<f32>>,
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
            flex_container: FlexContainerStyle {
                direction: self.flex_direction.map(|e| e.value).unwrap_or_default(),
                wrap: self.flex_wrap.map(|e| e.value).unwrap_or_default(),
                justify_content: self.justify_content.map(|e| e.value).unwrap_or_default(),
                align_items: self.align_items.map(|e| e.value).unwrap_or_default(),
                gap: self.gap.map(|e| e.value).unwrap_or(0.0),
                row_gap: self.row_gap.map(|e| e.value).unwrap_or(0.0),
                column_gap: self.column_gap.map(|e| e.value).unwrap_or(0.0),
            },
            flex_item: FlexItemStyle {
                flex_grow: self.flex_grow.map(|e| e.value).unwrap_or(0.0),
                flex_shrink: self.flex_shrink.map(|e| e.value).unwrap_or(1.0),
                flex_basis: self.flex_basis.map(|e| e.value).unwrap_or_default(),
                align_self: self.align_self.map(|e| e.value).unwrap_or_default(),
                order: self.order.map(|e| e.value).unwrap_or(0),
            },
            position: self.position.map(|e| e.value).unwrap_or_default(),
            top: self.top.map(|e| e.value).unwrap_or_default(),
            right: self.right_offset.map(|e| e.value).unwrap_or_default(),
            bottom: self.bottom.map(|e| e.value).unwrap_or_default(),
            left: self.left_offset.map(|e| e.value).unwrap_or_default(),
            z_index: self.z_index.map(|e| e.value).unwrap_or(0),
            overflow_x: self.overflow_x.map(|e| e.value).unwrap_or_default(),
            overflow_y: self.overflow_y.map(|e| e.value).unwrap_or_default(),
            opacity: self.opacity.map(|e| e.value).unwrap_or(1.0),
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
    tag_rules: FxHashMap<TagName, Vec<IndexedSelector>>,
    id_rules: FxHashMap<SelectorIdent, Vec<IndexedSelector>>,
    class_rules: FxHashMap<SelectorIdent, Vec<IndexedSelector>>,
    universal_rules: Vec<IndexedSelector>,
}

impl StyleIndex {
    fn new(stylesheet: &Stylesheet) -> Self {
        let mut index = StyleIndex {
            tag_rules: FxHashMap::default(),
            id_rules: FxHashMap::default(),
            class_rules: FxHashMap::default(),
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
) -> FxHashMap<NodeId, ComputedStyle> {
    let index = StyleIndex::new(stylesheet);
    let mut styles = FxHashMap::default();
    compute_styles_recursive(dom, root, stylesheet, &index, None, &mut styles);
    styles
}

// Used by silksurf-engine; not referenced internally in this crate yet.
#[allow(dead_code)]
pub struct StyleCache {
    generation: u64,
    styles: Arc<FxHashMap<NodeId, ComputedStyle>>,
}

#[allow(dead_code)]
impl StyleCache {
    pub fn new() -> Self {
        Self {
            generation: 0,
            styles: Arc::new(FxHashMap::default()),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn styles(&self) -> &FxHashMap<NodeId, ComputedStyle> {
        self.styles.as_ref()
    }

    pub fn styles_arc(&self) -> Arc<FxHashMap<NodeId, ComputedStyle>> {
        Arc::clone(&self.styles)
    }

    pub fn compute(
        &mut self,
        dom: &Dom,
        root: NodeId,
        stylesheet: &Stylesheet,
    ) -> Arc<FxHashMap<NodeId, ComputedStyle>> {
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
    ) -> Arc<FxHashMap<NodeId, ComputedStyle>> {
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
        let mut seen = FxHashSet::default();
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
    styles: &mut FxHashMap<NodeId, ComputedStyle>,
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
    let mut matched_by_rule: Vec<Option<Specificity>> = vec![None; stylesheet.rules.len()];
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

    let mut seen = FxHashSet::default();
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
        let Some(specificity) = matched_by_rule.get(rule_index).and_then(|spec| *spec) else {
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
        // Flex container properties
        "flex-direction" => {
            if let Some(value) = parse_flex_direction(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_direction,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "flex-wrap" => {
            if let Some(value) = parse_flex_wrap(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_wrap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "flex-flow" => {
            // Shorthand: flex-flow: <direction> <wrap>
            if let Some(dir) = parse_flex_direction(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_direction,
                    dir,
                    declaration.important,
                    specificity,
                    order,
                );
            }
            if let Some(wrap) = parse_flex_wrap(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_wrap,
                    wrap,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "justify-content" => {
            if let Some(value) = parse_justify_content(&declaration.value) {
                apply_property(
                    &mut cascaded.justify_content,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "align-items" => {
            if let Some(value) = parse_align_items(&declaration.value) {
                apply_property(
                    &mut cascaded.align_items,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "align-self" => {
            if let Some(value) = parse_align_self(&declaration.value) {
                apply_property(
                    &mut cascaded.align_self,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "gap" => {
            if let Some(value) = parse_gap_value(&declaration.value) {
                apply_property(
                    &mut cascaded.gap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
                apply_property(
                    &mut cascaded.row_gap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
                apply_property(
                    &mut cascaded.column_gap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "row-gap" => {
            if let Some(value) = parse_gap_value(&declaration.value) {
                apply_property(
                    &mut cascaded.row_gap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "column-gap" => {
            if let Some(value) = parse_gap_value(&declaration.value) {
                apply_property(
                    &mut cascaded.column_gap,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Flex item properties
        "flex-grow" => {
            if let Some(value) = parse_number_value(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_grow,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "flex-shrink" => {
            if let Some(value) = parse_number_value(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_shrink,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "flex-basis" => {
            if let Some(value) = parse_flex_basis(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_basis,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "flex" => {
            // Shorthand: flex: <grow> [<shrink>] [<basis>]
            let nums: Vec<f32> = declaration
                .value
                .iter()
                .filter_map(|t| match t {
                    CssToken::Number(v) => v.parse::<f32>().ok(),
                    _ => None,
                })
                .collect();
            if !nums.is_empty() {
                apply_property(
                    &mut cascaded.flex_grow,
                    nums[0],
                    declaration.important,
                    specificity,
                    order,
                );
                if nums.len() > 1 {
                    apply_property(
                        &mut cascaded.flex_shrink,
                        nums[1],
                        declaration.important,
                        specificity,
                        order,
                    );
                }
            }
            if let Some(basis) = parse_flex_basis(&declaration.value) {
                apply_property(
                    &mut cascaded.flex_basis,
                    basis,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "order" => {
            if let Some(value) = parse_integer_value(&declaration.value) {
                apply_property(
                    &mut cascaded.order,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Positioning
        "position" => {
            if let Some(value) = parse_position(&declaration.value) {
                apply_property(
                    &mut cascaded.position,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "top" => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.top,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "right" => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.right_offset,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "bottom" => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.bottom,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "left" => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.left_offset,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "z-index" => {
            if let Some(value) = parse_integer_value(&declaration.value) {
                apply_property(
                    &mut cascaded.z_index,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Overflow
        "overflow" => {
            if let Some(value) = parse_overflow(&declaration.value) {
                apply_property(
                    &mut cascaded.overflow_x,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
                apply_property(
                    &mut cascaded.overflow_y,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "overflow-x" => {
            if let Some(value) = parse_overflow(&declaration.value) {
                apply_property(
                    &mut cascaded.overflow_x,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        "overflow-y" => {
            if let Some(value) = parse_overflow(&declaration.value) {
                apply_property(
                    &mut cascaded.overflow_y,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Visual
        "opacity" => {
            if let Some(value) = parse_opacity(&declaration.value) {
                apply_property(
                    &mut cascaded.opacity,
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
        "flex" => Some(Display::Flex),
        "inline-flex" => Some(Display::InlineFlex),
        "none" => Some(Display::None),
        _ => None,
    }
}

fn parse_flex_direction(tokens: &[CssToken]) -> Option<FlexDirection> {
    let ident = first_ident(tokens)?;
    match ident {
        "row" => Some(FlexDirection::Row),
        "row-reverse" => Some(FlexDirection::RowReverse),
        "column" => Some(FlexDirection::Column),
        "column-reverse" => Some(FlexDirection::ColumnReverse),
        _ => None,
    }
}

fn parse_flex_wrap(tokens: &[CssToken]) -> Option<FlexWrap> {
    let ident = first_ident(tokens)?;
    match ident {
        "nowrap" => Some(FlexWrap::Nowrap),
        "wrap" => Some(FlexWrap::Wrap),
        "wrap-reverse" => Some(FlexWrap::WrapReverse),
        _ => None,
    }
}

fn parse_justify_content(tokens: &[CssToken]) -> Option<JustifyContent> {
    let ident = first_ident(tokens)?;
    match ident {
        "flex-start" | "start" => Some(JustifyContent::FlexStart),
        "flex-end" | "end" => Some(JustifyContent::FlexEnd),
        "center" => Some(JustifyContent::Center),
        "space-between" => Some(JustifyContent::SpaceBetween),
        "space-around" => Some(JustifyContent::SpaceAround),
        "space-evenly" => Some(JustifyContent::SpaceEvenly),
        _ => None,
    }
}

fn parse_align_items(tokens: &[CssToken]) -> Option<AlignItems> {
    let ident = first_ident(tokens)?;
    match ident {
        "stretch" => Some(AlignItems::Stretch),
        "flex-start" | "start" => Some(AlignItems::FlexStart),
        "flex-end" | "end" => Some(AlignItems::FlexEnd),
        "center" => Some(AlignItems::Center),
        "baseline" => Some(AlignItems::Baseline),
        _ => None,
    }
}

fn parse_align_self(tokens: &[CssToken]) -> Option<AlignSelf> {
    let ident = first_ident(tokens)?;
    match ident {
        "auto" => Some(AlignSelf::Auto),
        "flex-start" | "start" => Some(AlignSelf::FlexStart),
        "flex-end" | "end" => Some(AlignSelf::FlexEnd),
        "center" => Some(AlignSelf::Center),
        "stretch" => Some(AlignSelf::Stretch),
        "baseline" => Some(AlignSelf::Baseline),
        _ => None,
    }
}

fn parse_flex_basis(tokens: &[CssToken]) -> Option<FlexBasis> {
    let ident = first_ident(tokens);
    if ident == Some("auto") {
        return Some(FlexBasis::Auto);
    }
    parse_length(tokens).map(FlexBasis::Length)
}

fn parse_number_value(tokens: &[CssToken]) -> Option<f32> {
    tokens.iter().find_map(|token| match token {
        CssToken::Number(value) => value.parse::<f32>().ok(),
        _ => None,
    })
}

fn parse_integer_value(tokens: &[CssToken]) -> Option<i32> {
    tokens.iter().find_map(|token| match token {
        CssToken::Number(value) => value.parse::<i32>().ok(),
        _ => None,
    })
}

fn parse_gap_value(tokens: &[CssToken]) -> Option<f32> {
    tokens.iter().find_map(|token| match token {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok()
        }
        CssToken::Number(value) if value == "0" => Some(0.0),
        _ => None,
    })
}

fn first_ident(tokens: &[CssToken]) -> Option<&str> {
    tokens.iter().find_map(|token| match token {
        CssToken::Ident(value) => Some(value.as_str()),
        _ => None,
    })
}

fn parse_length(tokens: &[CssToken]) -> Option<Length> {
    tokens.iter().find_map(parse_length_token)
}

fn parse_length_token(token: &CssToken) -> Option<Length> {
    match token {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok().map(Length::Px)
        }
        CssToken::Percentage(value) => value.parse::<f32>().ok().map(Length::Percent),
        CssToken::Number(value) if value == "0" => Some(Length::zero()),
        _ => None,
    }
}

fn parse_position(tokens: &[CssToken]) -> Option<Position> {
    match first_ident(tokens)? {
        "static" => Some(Position::Static),
        "relative" => Some(Position::Relative),
        "absolute" => Some(Position::Absolute),
        "fixed" => Some(Position::Fixed),
        "sticky" => Some(Position::Sticky),
        _ => None,
    }
}

fn parse_overflow(tokens: &[CssToken]) -> Option<Overflow> {
    match first_ident(tokens)? {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}

fn parse_length_or_auto(tokens: &[CssToken]) -> Option<LengthOrAuto> {
    if first_ident(tokens) == Some("auto") {
        return Some(LengthOrAuto::Auto);
    }
    parse_length(tokens).map(LengthOrAuto::Length)
}

fn parse_opacity(tokens: &[CssToken]) -> Option<f32> {
    tokens.iter().find_map(|token| match token {
        CssToken::Number(value) => value.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0)),
        _ => None,
    })
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
    let mut iter = tokens
        .iter()
        .filter(|token| !matches!(token, CssToken::Whitespace));
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
