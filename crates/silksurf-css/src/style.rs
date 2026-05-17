/*
 * style.rs -- CSS cascade algorithm, computed style resolution, property parsing.
 *
 * WHY: Implements CSS Cascading and Inheritance Level 4. For each DOM node,
 * determines the final computed value of every CSS property by collecting
 * matching selectors, sorting by specificity, and resolving cascade conflicts
 * (important > specificity > source order).
 *
 * Architecture:
 *   1. StyleIndex: hash-based index of selectors by tag/class/id (O(1) lookup)
 *   2. cascade_for_node: collects matching rules, builds CascadedStyle
 *   3. CascadedStyle::resolve: inherits from parent, fills defaults
 *   4. apply_declaration: parses CSS value tokens into typed values
 *
 * Performance (ChatGPT: 401 nodes, 33 rules):
 *   StyleIndex construction: O(rules * selectors) -- one-time at parse
 *   Per-node cascade: O(matching_rules) -- typically 5-20 rules
 *   Total: O(nodes * avg_matching_rules) -- ~8000 operations
 *
 * DONE(perf): Property ID interning (Phase 4.2) -- see property_id.rs
 * TODO(perf): SoA conversion for 16x cache reuse (Phase 4.4)
 *
 * See: matching.rs for selector matching, selector.rs for parsing
 * See: custom_properties.rs for CSS var() resolution
 * See: calc.rs for calc() expression evaluation
 */
use crate::matching::{
    Specificity, matches_selector, matches_selector_with_view, selector_specificity,
};
use crate::selector::{Selector, SelectorIdent, SelectorModifier, TypeSelector};
use crate::{CssToken, Declaration, Rule, Stylesheet};
use rustc_hash::{FxHashMap, FxHashSet};
use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind, TagName};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Inline,
    Block,
    Flex,
    InlineFlex,
    Grid,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontWeight {
    Normal,
    Bold,
    Bolder,
    Lighter,
    Number(u16),
}

impl Default for FontWeight {
    fn default() -> Self {
        FontWeight::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Overline,
    LineThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub color: Color,
    pub inset: bool,
}

/// CSS linear-gradient() value.
///
/// angle_deg follows CSS convention: 0 = to-top, 90 = to-right, 180 = to-bottom.
/// stops positions are in the range [0.0, 1.0] (0% to 100%).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    pub angle_deg: f32,
    pub stops: Vec<(f32, Color)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
    /// Relative to the element's own computed font-size (resolved at cascade time).
    Em(f32),
    /// Relative to the root element's font-size; defaults to 16 px (resolved at cascade time).
    Rem(f32),
}

impl Length {
    pub fn zero() -> Self {
        Length::Px(0.0)
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Length::Px(v) | Length::Percent(v) | Length::Em(v) | Length::Rem(v) => *v == 0.0,
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

/// Per-side margin values, each optionally `auto`.
///
/// Distinct from `Edges` because margin supports `auto` (used for centering),
/// while padding and border widths only accept non-negative lengths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top: LengthOrAuto,
    pub right: LengthOrAuto,
    pub bottom: LengthOrAuto,
    pub left: LengthOrAuto,
}

impl Margins {
    pub fn all(v: LengthOrAuto) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub fn zero() -> Self {
        Self::all(LengthOrAuto::Length(Length::zero()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Color,
    pub background_color: Color,
    pub font_size: Length,
    pub line_height: Length,
    pub font_family: SmallVec<[SmolStr; 2]>,
    pub margin: Margins,
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
    // Sizing
    pub width: LengthOrAuto,
    pub height: LengthOrAuto,
    pub min_width: Length,
    pub max_width: Option<Length>,
    pub min_height: Length,
    pub max_height: Option<Length>,
    // Overflow
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    // Border rendering
    pub border_color: Color,
    pub border_style: BorderStyle,
    // Visual
    pub opacity: f32,
    pub visibility: Visibility,
    // Text
    pub text_align: TextAlign,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub text_decoration: TextDecoration,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub white_space: WhiteSpace,
    // Decoration
    pub border_radius: f32,
    pub box_shadow: Option<BoxShadow>,
    pub background_image: Option<LinearGradient>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            color: Color::black(),
            background_color: Color::transparent(),
            font_size: Length::Px(16.0),
            line_height: Length::Px(16.0),
            font_family: {
                let mut v = SmallVec::<[SmolStr; 2]>::new();
                v.push(SmolStr::new_static("sans-serif"));
                v
            },
            margin: Margins::zero(),
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
            width: LengthOrAuto::Auto,
            height: LengthOrAuto::Auto,
            min_width: Length::Px(0.0),
            max_width: None,
            min_height: Length::Px(0.0),
            max_height: None,
            overflow_x: Overflow::default(),
            overflow_y: Overflow::default(),
            border_color: Color::black(),
            border_style: BorderStyle::default(),
            opacity: 1.0,
            visibility: Visibility::default(),
            text_align: TextAlign::default(),
            font_weight: FontWeight::default(),
            font_style: FontStyle::default(),
            text_decoration: TextDecoration::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            white_space: WhiteSpace::default(),
            border_radius: 0.0,
            box_shadow: None,
            background_image: None,
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
    font_family: Option<ResolvedProperty<SmallVec<[SmolStr; 2]>>>,
    margin_top: Option<ResolvedProperty<LengthOrAuto>>,
    margin_right: Option<ResolvedProperty<LengthOrAuto>>,
    margin_bottom: Option<ResolvedProperty<LengthOrAuto>>,
    margin_left: Option<ResolvedProperty<LengthOrAuto>>,
    padding_top: Option<ResolvedProperty<Length>>,
    padding_right: Option<ResolvedProperty<Length>>,
    padding_bottom: Option<ResolvedProperty<Length>>,
    padding_left: Option<ResolvedProperty<Length>>,
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
    // Sizing
    width: Option<ResolvedProperty<LengthOrAuto>>,
    height: Option<ResolvedProperty<LengthOrAuto>>,
    min_width: Option<ResolvedProperty<Length>>,
    max_width: Option<ResolvedProperty<Option<Length>>>,
    min_height: Option<ResolvedProperty<Length>>,
    max_height: Option<ResolvedProperty<Option<Length>>>,
    // Overflow
    overflow_x: Option<ResolvedProperty<Overflow>>,
    overflow_y: Option<ResolvedProperty<Overflow>>,
    // Border rendering
    border_color: Option<ResolvedProperty<Color>>,
    border_style: Option<ResolvedProperty<BorderStyle>>,
    // Visual
    opacity: Option<ResolvedProperty<f32>>,
    visibility: Option<ResolvedProperty<Visibility>>,
    // Text
    text_align: Option<ResolvedProperty<TextAlign>>,
    font_weight: Option<ResolvedProperty<FontWeight>>,
    font_style: Option<ResolvedProperty<FontStyle>>,
    text_decoration: Option<ResolvedProperty<TextDecoration>>,
    letter_spacing: Option<ResolvedProperty<f32>>,
    word_spacing: Option<ResolvedProperty<f32>>,
    white_space: Option<ResolvedProperty<WhiteSpace>>,
    // Decoration
    border_radius: Option<ResolvedProperty<f32>>,
    box_shadow: Option<ResolvedProperty<BoxShadow>>,
    background_image: Option<ResolvedProperty<LinearGradient>>,
}

/*
 * em/rem resolution helpers -- convert relative length units to absolute px.
 *
 * WHY: Em and Rem units are relative (em = element font-size, rem = root
 * font-size). They are stored as-is during parsing and resolved here, in
 * the cascade resolve pass, so all downstream code (layout, render) only
 * ever sees Px or Percent values in ComputedStyle.
 *
 * font-size uses parent_font_size_px as its em base (self-referential em
 * for font-size means relative to parent, per CSS spec). All other properties
 * use the element's own resolved font-size as the em base.
 */
fn resolve_length(l: Length, em_px: f32, rem_px: f32) -> Length {
    match l {
        Length::Px(_) | Length::Percent(_) => l,
        Length::Em(multiplier) => Length::Px(multiplier * em_px),
        Length::Rem(multiplier) => Length::Px(multiplier * rem_px),
    }
}

fn resolve_edges(edges: Edges, em_px: f32, rem_px: f32) -> Edges {
    Edges {
        top: resolve_length(edges.top, em_px, rem_px),
        right: resolve_length(edges.right, em_px, rem_px),
        bottom: resolve_length(edges.bottom, em_px, rem_px),
        left: resolve_length(edges.left, em_px, rem_px),
    }
}

fn resolve_length_or_auto(l: LengthOrAuto, em_px: f32, rem_px: f32) -> LengthOrAuto {
    match l {
        LengthOrAuto::Auto => LengthOrAuto::Auto,
        LengthOrAuto::Length(len) => LengthOrAuto::Length(resolve_length(len, em_px, rem_px)),
    }
}

fn resolve_opt_length(l: Option<Length>, em_px: f32, rem_px: f32) -> Option<Length> {
    l.map(|len| resolve_length(len, em_px, rem_px))
}

fn resolve_margins(
    top: LengthOrAuto,
    right: LengthOrAuto,
    bottom: LengthOrAuto,
    left: LengthOrAuto,
    em_px: f32,
    rem_px: f32,
) -> Margins {
    Margins {
        top: resolve_length_or_auto(top, em_px, rem_px),
        right: resolve_length_or_auto(right, em_px, rem_px),
        bottom: resolve_length_or_auto(bottom, em_px, rem_px),
        left: resolve_length_or_auto(left, em_px, rem_px),
    }
}

impl CascadedStyle {
    /*
     * resolve -- produce final ComputedStyle from cascaded values + parent inheritance.
     *
     * WHY static FALLBACK: Previously constructed ComputedStyle::default() per
     * call (61 times per render). Each construction built a SmallVec + SmolStr.
     * With LazyLock, the default is constructed once and reused via reference.
     * Copy fields use the static directly; non-Copy (font_family) clones only
     * when needed (rare: only when no cascade value and no parent inheritance).
     */
    fn resolve(self, parent: Option<&ComputedStyle>, rem_base_px: f32) -> ComputedStyle {
        static FALLBACK: std::sync::LazyLock<ComputedStyle> =
            std::sync::LazyLock::new(ComputedStyle::default);
        let fallback = &*FALLBACK;

        // font-size: em is relative to the *parent* font-size (CSS spec).
        let parent_font_size_px = parent
            .map(|s| match s.font_size {
                Length::Px(v) => v,
                _ => 16.0,
            })
            .unwrap_or(16.0);
        let raw_font_size = self
            .font_size
            .map(|entry| entry.value)
            .or_else(|| parent.map(|s| s.font_size))
            .unwrap_or(fallback.font_size);
        // For font-size: Percent means percent of parent font-size (same as em).
        let resolved_font_size = match raw_font_size {
            Length::Em(m) => Length::Px(m * parent_font_size_px),
            Length::Rem(m) => Length::Px(m * rem_base_px),
            Length::Percent(p) => Length::Px(p / 100.0 * parent_font_size_px),
            other => other,
        };
        // All non-font-size length properties use the element's own font-size as em base.
        let em_px = match resolved_font_size {
            Length::Px(v) => v,
            _ => 16.0,
        };

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
            line_height: resolve_length(
                self.line_height
                    .map(|entry| entry.value)
                    .or_else(|| parent.map(|style| style.line_height))
                    .unwrap_or(resolved_font_size),
                em_px,
                rem_base_px,
            ),
            font_family: self
                .font_family
                .map(|entry| entry.value)
                .or_else(|| parent.map(|style| style.font_family.clone()))
                .unwrap_or_else(|| fallback.font_family.clone()),
            margin: {
                let zero = LengthOrAuto::Length(Length::Px(0.0));
                resolve_margins(
                    self.margin_top.map(|e| e.value).unwrap_or(zero),
                    self.margin_right.map(|e| e.value).unwrap_or(zero),
                    self.margin_bottom.map(|e| e.value).unwrap_or(zero),
                    self.margin_left.map(|e| e.value).unwrap_or(zero),
                    em_px,
                    rem_base_px,
                )
            },
            padding: resolve_edges(
                Edges {
                    top: self.padding_top.map(|e| e.value).unwrap_or(Length::Px(0.0)),
                    right: self
                        .padding_right
                        .map(|e| e.value)
                        .unwrap_or(Length::Px(0.0)),
                    bottom: self
                        .padding_bottom
                        .map(|e| e.value)
                        .unwrap_or(Length::Px(0.0)),
                    left: self
                        .padding_left
                        .map(|e| e.value)
                        .unwrap_or(Length::Px(0.0)),
                },
                em_px,
                rem_base_px,
            ),
            border: resolve_edges(
                self.border
                    .map(|entry| entry.value)
                    .unwrap_or(fallback.border),
                em_px,
                rem_base_px,
            ),
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
            top: resolve_length_or_auto(
                self.top.map(|e| e.value).unwrap_or_default(),
                em_px,
                rem_base_px,
            ),
            right: resolve_length_or_auto(
                self.right_offset.map(|e| e.value).unwrap_or_default(),
                em_px,
                rem_base_px,
            ),
            bottom: resolve_length_or_auto(
                self.bottom.map(|e| e.value).unwrap_or_default(),
                em_px,
                rem_base_px,
            ),
            left: resolve_length_or_auto(
                self.left_offset.map(|e| e.value).unwrap_or_default(),
                em_px,
                rem_base_px,
            ),
            z_index: self.z_index.map(|e| e.value).unwrap_or(0),
            width: resolve_length_or_auto(
                self.width.map(|e| e.value).unwrap_or(LengthOrAuto::Auto),
                em_px,
                rem_base_px,
            ),
            height: resolve_length_or_auto(
                self.height.map(|e| e.value).unwrap_or(LengthOrAuto::Auto),
                em_px,
                rem_base_px,
            ),
            min_width: resolve_length(
                self.min_width.map(|e| e.value).unwrap_or(Length::Px(0.0)),
                em_px,
                rem_base_px,
            ),
            max_width: resolve_opt_length(
                self.max_width.map(|e| e.value).unwrap_or(None),
                em_px,
                rem_base_px,
            ),
            min_height: resolve_length(
                self.min_height.map(|e| e.value).unwrap_or(Length::Px(0.0)),
                em_px,
                rem_base_px,
            ),
            max_height: resolve_opt_length(
                self.max_height.map(|e| e.value).unwrap_or(None),
                em_px,
                rem_base_px,
            ),
            overflow_x: self.overflow_x.map(|e| e.value).unwrap_or_default(),
            overflow_y: self.overflow_y.map(|e| e.value).unwrap_or_default(),
            border_color: self
                .border_color
                .map(|e| e.value)
                .unwrap_or_else(Color::black),
            border_style: self.border_style.map(|e| e.value).unwrap_or_default(),
            opacity: self.opacity.map(|e| e.value).unwrap_or(1.0),
            visibility: self.visibility.map(|e| e.value).unwrap_or_default(),
            text_align: self
                .text_align
                .map(|e| e.value)
                .or_else(|| parent.map(|s| s.text_align))
                .unwrap_or_default(),
            font_weight: self
                .font_weight
                .map(|e| e.value)
                .or_else(|| parent.map(|s| s.font_weight))
                .unwrap_or_default(),
            font_style: self
                .font_style
                .map(|e| e.value)
                .or_else(|| parent.map(|s| s.font_style))
                .unwrap_or_default(),
            text_decoration: self
                .text_decoration
                .map(|e| e.value)
                .or_else(|| parent.map(|s| s.text_decoration))
                .unwrap_or_default(),
            letter_spacing: self.letter_spacing.map(|e| e.value).unwrap_or(0.0),
            word_spacing: self.word_spacing.map(|e| e.value).unwrap_or(0.0),
            white_space: self
                .white_space
                .map(|e| e.value)
                .or_else(|| parent.map(|s| s.white_space))
                .unwrap_or_default(),
            border_radius: self.border_radius.map(|e| e.value).unwrap_or(0.0),
            box_shadow: self.box_shadow.map(|e| e.value),
            background_image: self.background_image.map(|e| e.value),
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

/*
 * IndexedSelector -- pre-indexed selector entry for cascade candidate lookup.
 *
 * pair_id: sequential index assigned by StyleIndex::new(), unique per
 * (rule_index, selector_index) pair. Used as a bit index into the
 * CascadeWorkspace::seen_bits bitvec for O(1) dedup with zero hashing.
 */
#[derive(Clone)]
struct IndexedSelector {
    rule_index: usize,
    selector_index: usize,
    specificity: Specificity,
    pair_id: u32,
}

/*
 * StyleIndex -- hash-based selector index for O(1) candidate lookup.
 *
 * WHY: Naive cascade iterates ALL selectors for EVERY node: O(N*S).
 * StyleIndex partitions selectors by their rightmost simple selector
 * (tag, id, class, or universal) into hash maps. For each node, we
 * only check selectors whose key matches the node's tag/id/classes.
 *
 * Built once at stylesheet parse time. For ChatGPT (33 rules, ~50 selectors):
 *   tag_rules: ~20 entries (div, span, a, etc.)
 *   id_rules: ~5 entries
 *   class_rules: ~15 entries
 *   universal_rules: ~5 entries
 *
 * Analogous to the gororoba NeighborTable which pre-computes cell
 * relationships to eliminate modular arithmetic from the hot loop.
 * See: gororoba_app/crates/gororoba_bevy_lbm/src/soa_solver.rs:100
 *
 * See: cascade_for_node() for how the index is queried per node
 * See: selector_key() for rightmost-selector extraction
 */
pub struct StyleIndex {
    tag_rules: FxHashMap<TagName, Vec<IndexedSelector>>,
    id_rules: FxHashMap<SelectorIdent, Vec<IndexedSelector>>,
    class_rules: FxHashMap<SelectorIdent, Vec<IndexedSelector>>,
    universal_rules: Vec<IndexedSelector>,
    /// Total number of unique (rule, selector) pairs. Used to size the
    /// CascadeWorkspace::seen_bits bitvec for O(1) dedup without hashing.
    pub total_selector_pairs: usize,
}

impl StyleIndex {
    pub fn new(stylesheet: &Stylesheet) -> Self {
        let mut index = StyleIndex {
            tag_rules: FxHashMap::default(),
            id_rules: FxHashMap::default(),
            class_rules: FxHashMap::default(),
            universal_rules: Vec::new(),
            total_selector_pairs: 0,
        };
        let mut pair_id: u32 = 0;
        for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
            let Rule::Style(rule) = rule else {
                continue;
            };
            for (selector_index, selector) in rule.selectors.selectors.iter().enumerate() {
                let entry = IndexedSelector {
                    rule_index,
                    selector_index,
                    specificity: selector_specificity(selector),
                    pair_id,
                };
                pair_id += 1;
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
        index.total_selector_pairs = pair_id as usize;
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

/*
 * node_tag_id_class -- fused DOM access: tag name, id key, and class keys.
 *
 * WHY: The previous two functions (node_tag, node_id_class_keys) each
 * called dom.node() and iterated attributes separately -- two lookups and
 * two attribute scans per node. This fuses them into one dom.node() call.
 *
 * Fix 3: Uses attr.class_strings (pre-resolved at set_attribute time)
 * instead of dom.resolve(atom) + RwLock acquire per class token.
 * For id: attr.value already holds the raw string, no resolve needed.
 *
 * Fix 2: class_keys is a &mut Vec from the workspace, reused across nodes.
 * Previously a fresh Vec was allocated per call (~303ns overhead at 61 nodes).
 *
 * Fix F: Single dom.node() call replaces two (node_tag + node_id_class_keys).
 *
 * Complexity: O(attributes) per node -- typically 1-3 attributes.
 */
fn node_tag_id_class(
    dom: &Dom,
    node: NodeId,
    class_keys: &mut Vec<SelectorIdent>,
) -> (Option<TagName>, Option<SelectorIdent>) {
    class_keys.clear();
    let Ok(node_ref) = dom.node(node) else {
        return (None, None);
    };
    let (tag, attributes) = match node_ref.kind() {
        NodeKind::Element {
            name, attributes, ..
        } => (Some(name.clone()), attributes.as_slice()),
        _ => return (None, None),
    };
    let mut id_key = None;
    for attr in attributes {
        match attr.name {
            AttributeName::Id => {
                if let Some(atom) = attr.value_atom {
                    id_key = Some(SelectorIdent::new_with_atom(attr.value.clone(), atom));
                } else if !attr.value.is_empty() {
                    id_key = Some(SelectorIdent::from(attr.value.clone()));
                }
            }
            AttributeName::Class => {
                if !attr.class_strings.is_empty() {
                    for (s, &atom) in attr.class_strings.iter().zip(attr.value_atoms.iter()) {
                        class_keys.push(SelectorIdent::new_with_atom(s.clone(), atom));
                    }
                } else if !attr.value_atoms.is_empty() {
                    // Fallback: atoms without pre-resolved class_strings.
                    // Uses resolve_fast (lock-free array index) instead of
                    // resolve (RwLock acquire) when resolve table is materialized.
                    for &atom in &attr.value_atoms {
                        class_keys.push(SelectorIdent::new_with_atom(
                            dom.resolve_fast(atom).clone(),
                            atom,
                        ));
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
    (tag, id_key)
}

/*
 * CascadeWorkspace -- reusable scratch buffers for cascade_for_node.
 *
 * WHY: cascade_for_node previously allocated three heap objects per call:
 *   matched_by_rule: Vec<Option<Specificity>>  (len = rules count)
 *   candidates: Vec<IndexedSelector>           (capacity grows to peak)
 *   seen: FxHashSet<(usize, usize)>            (capacity grows to peak)
 *
 * For N nodes that is 3*N allocations. With a shared workspace passed
 * through the traversal, all post-first-call allocations are eliminated:
 * prepare() zero-fills matched_by_rule in-place (O(rules)), candidates
 * and seen_bits are cleared (O(1) capacity retained, no heap traffic).
 *
 * Fix D: FxHashSet<(usize,usize)> replaced with Vec<u64> bitvec indexed
 * by IndexedSelector::pair_id. For 159 pairs: 3 u64 words = 24 bytes
 * cleared via fill(0) (memset) vs FxHashSet bucket traversal. Saves ~1.5us.
 *
 * Fix 2: class_keys Vec<SelectorIdent> is reused across nodes instead
 * of allocating a new Vec per node_id_class_keys call. Saves ~0.3us.
 *
 * High-water-mark growth: the workspace capacity grows to the largest
 * rule count / candidate count seen and never shrinks. After the first
 * stylesheet pass, no further allocations occur in the hot loop.
 *
 * Usage: create once per stylesheet, thread as &mut through cascade loop.
 * See: cascade_for_node() for usage
 * See: compute_styles() for where workspace is created
 */
pub struct CascadeWorkspace {
    matched_by_rule: Vec<Option<Specificity>>,
    candidates: Vec<IndexedSelector>,
    /// Bitvec tracking visited (rule, selector) pairs via pair_id.
    /// Word i covers pair_ids [64*i..64*(i+1)). Cleared via fill(0) in prepare().
    seen_bits: Vec<u64>,
    /// Reusable scratch for class key collection per node (Fix 2).
    class_keys: Vec<SelectorIdent>,
}

impl CascadeWorkspace {
    pub fn new(rules_len: usize) -> Self {
        Self {
            matched_by_rule: vec![None; rules_len],
            candidates: Vec::with_capacity(32),
            seen_bits: Vec::new(),
            class_keys: Vec::with_capacity(8),
        }
    }

    /*
     * prepare -- reset scratch buffers for the next node, no allocation.
     *
     * Zero-fills matched_by_rule[..rules_len] and seen_bits[..words_needed].
     * Both use high-water-mark growth: if the new stylesheet has more rules
     * or selector pairs, the Vecs grow; otherwise they reuse existing capacity.
     * candidates and class_keys are cleared with O(1) capacity-preserving clear().
     */
    fn prepare(&mut self, rules_len: usize, total_selector_pairs: usize) {
        if self.matched_by_rule.len() < rules_len {
            self.matched_by_rule.resize(rules_len, None);
        } else {
            self.matched_by_rule[..rules_len].fill(None);
        }
        let words_needed = total_selector_pairs.div_ceil(64);
        if self.seen_bits.len() < words_needed {
            self.seen_bits.resize(words_needed, 0);
        } else if words_needed > 0 {
            self.seen_bits[..words_needed].fill(0);
        }
        self.candidates.clear();
        self.class_keys.clear();
    }
}

/*
 * compute_styles -- compute CSS styles for all nodes in a DOM subtree.
 *
 * WHY: Entry point for the style pipeline. Builds a StyleIndex from the
 * stylesheet (one-time O(selectors)), then walks the DOM depth-first,
 * computing styles per node with inheritance from parent.
 *
 * Returns FxHashMap<NodeId, ComputedStyle> mapping every node to its
 * resolved style. This map is consumed by the layout engine.
 *
 * Complexity: O(N * R_avg) where N=nodes, R_avg=matching rules per node
 * Memory: O(N * sizeof(ComputedStyle)) for the result map
 *
 * See: StyleIndex::new() for selector index construction
 * See: compute_styles_recursive() for depth-first tree walk
 * See: cascade_for_node() for per-node cascade resolution
 */
pub fn compute_styles(
    dom: &Dom,
    root: NodeId,
    stylesheet: &Stylesheet,
) -> FxHashMap<NodeId, ComputedStyle> {
    let index = StyleIndex::new(stylesheet);
    let mut workspace = CascadeWorkspace::new(stylesheet.rules.len());
    let mut styles = FxHashMap::default();
    compute_styles_recursive(
        dom,
        root,
        stylesheet,
        &index,
        None,
        &mut styles,
        &mut workspace,
        16.0,
    );
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
        let mut workspace = CascadeWorkspace::new(stylesheet.rules.len());
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
                &mut workspace,
                16.0,
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
    let mut workspace = CascadeWorkspace::new(stylesheet.rules.len());
    compute_style_for_node_with_workspace(
        dom,
        node,
        stylesheet,
        &index,
        parent,
        &mut workspace,
        None,
        16.0,
    )
}

pub fn compute_style_for_node_with_index(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    parent: Option<&ComputedStyle>,
) -> ComputedStyle {
    let mut workspace = CascadeWorkspace::new(stylesheet.rules.len());
    compute_style_for_node_with_workspace(
        dom,
        node,
        stylesheet,
        index,
        parent,
        &mut workspace,
        None,
        16.0,
    )
}

/*
 * compute_style_for_node_with_workspace -- cascade for one node, workspace reused.
 *
 * WHY: The fused pipeline calls this once per node in BFS order. With a
 * shared CascadeWorkspace, the three per-node allocations (matched_by_rule,
 * candidates, seen) are eliminated after the first call. Pass the same
 * workspace from fused_style_layout_paint's BFS loop for zero alloc overhead.
 *
 * See: CascadeWorkspace for scratch buffer lifecycle
 * See: fused_pipeline.rs fused_style_layout_paint() for call site
 */
#[allow(clippy::too_many_arguments)]
pub fn compute_style_for_node_with_workspace(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    parent: Option<&ComputedStyle>,
    workspace: &mut CascadeWorkspace,
    cascade_view: Option<&crate::cascade_view::CascadeView>,
    rem_base_px: f32,
) -> ComputedStyle {
    if dom.element_name(node).ok().flatten().is_none() {
        return parent.cloned().unwrap_or_default();
    }
    cascade_for_node(dom, node, stylesheet, index, workspace, cascade_view)
        .resolve(parent, rem_base_px)
}

#[allow(clippy::too_many_arguments)]
fn compute_styles_recursive(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    parent: Option<&ComputedStyle>,
    styles: &mut FxHashMap<NodeId, ComputedStyle>,
    workspace: &mut CascadeWorkspace,
    rem_base_px: f32,
) {
    let style = compute_style_for_node_with_workspace(
        dom,
        node,
        stylesheet,
        index,
        parent,
        workspace,
        None,
        rem_base_px,
    );
    styles.insert(node, style.clone());
    // Update rem_base after processing the html element: all descendants
    // use its resolved font-size as the rem base (CSS spec rem = root em).
    let child_rem_base = if dom
        .element_name(node)
        .ok()
        .flatten()
        .map(|name| name.eq_ignore_ascii_case("html"))
        .unwrap_or(false)
    {
        match style.font_size {
            Length::Px(v) => v,
            _ => rem_base_px,
        }
    } else {
        rem_base_px
    };
    if let Ok(children) = dom.children(node) {
        for child in children {
            compute_styles_recursive(
                dom,
                *child,
                stylesheet,
                index,
                Some(&style),
                styles,
                workspace,
                child_rem_base,
            );
        }
    }
}

/*
 * cascade_for_node -- resolve CSS cascade for a single DOM node.
 *
 * WHY: Collects all matching selectors from the StyleIndex, deduplicates
 * by (rule_index, selector_index), then applies declarations in source
 * order with specificity-based override per CSS Cascade Level 4 Section 6.
 *
 * Algorithm:
 *   1. Lookup node's tag/id/classes in StyleIndex hash maps
 *   2. Collect candidate selectors (may have duplicates from multiple maps)
 *   3. Deduplicate via HashSet<(rule_idx, selector_idx)>
 *   4. For each unique candidate, verify full selector match
 *   5. Track highest specificity per rule (for multi-selector rules)
 *   6. Apply declarations in source order, respecting specificity
 *
 * Complexity: O(C + M) where C=candidates from index, M=matching rules
 * Memory: allocates matched_by_rule Vec<Option<Specificity>> per node
 *
 * INVARIANT: specificity ordering preserved by apply_property's
 * should_override check -- !important > specificity > source order
 *
 * See: StyleIndex for candidate lookup
 * See: matches_selector (matching.rs) for full selector verification
 * See: apply_declaration for property value parsing
 */
fn cascade_for_node(
    dom: &Dom,
    node: NodeId,
    stylesheet: &Stylesheet,
    index: &StyleIndex,
    workspace: &mut CascadeWorkspace,
    cascade_view: Option<&crate::cascade_view::CascadeView>,
) -> CascadedStyle {
    /*
     * Prepare workspace: zero-fill matched_by_rule, zero seen_bits, clear
     * candidates and class_keys. No heap allocation after steady state.
     * See: CascadeWorkspace::prepare() for invariant details.
     */
    workspace.prepare(stylesheet.rules.len(), index.total_selector_pairs);

    let mut cascaded = CascadedStyle::default();
    let mut order = 0usize;

    /*
     * Candidate collection: two paths depending on whether CascadeView
     * is available.
     *
     * CascadeView path (SoA, single cache line per node):
     *   Reads CascadeEntry (36 bytes) + pre-constructed SelectorIdents
     *   from flat array. No dom.node() call, no attribute iteration,
     *   no SelectorIdent construction. Each node touches 1 cache line
     *   for the entry + sequential ident reads.
     *
     * Fallback path (AoS, 2.6 cache lines per node):
     *   Calls node_tag_id_class -> dom.node() -> pattern match -> attr scan.
     *   Used when CascadeView is not materialized (e.g., compute_styles
     *   without FusedWorkspace).
     */
    if let Some(view) = cascade_view {
        let entry = &view.entries[node.raw()];
        if let Some(entries) = index.tag_rules.get(&entry.tag) {
            workspace.candidates.extend_from_slice(entries);
        }
        if let Some(id_ident) = view.id_ident(entry) {
            if let Some(entries) = index.id_rules.get(id_ident) {
                workspace.candidates.extend_from_slice(entries);
            }
        }
        for class_ident in view.class_idents(entry) {
            if let Some(entries) = index.class_rules.get(class_ident) {
                workspace.candidates.extend_from_slice(entries);
            }
        }
    } else {
        let (tag, id_key) = node_tag_id_class(dom, node, &mut workspace.class_keys);
        if let Some(tag) = tag {
            if let Some(entries) = index.tag_rules.get(&tag) {
                workspace.candidates.extend_from_slice(entries);
            }
        }
        if let Some(ref id_key) = id_key {
            if let Some(entries) = index.id_rules.get(id_key) {
                workspace.candidates.extend_from_slice(entries);
            }
        }
        for class_key in &workspace.class_keys {
            if let Some(entries) = index.class_rules.get(class_key) {
                workspace.candidates.extend_from_slice(entries);
            }
        }
    }
    workspace
        .candidates
        .extend_from_slice(&index.universal_rules);

    /*
     * Take candidates and seen_bits out of workspace so we can mutate
     * workspace.matched_by_rule inside the loop without borrow conflicts.
     * mem::take leaves empty Vecs behind (zero allocation -- the original
     * allocation is now owned by the locals). Returned after the loop.
     *
     * Fix D: seen_bits bitvec replaces FxHashSet for O(1) dedup via pair_id.
     * fill(0) in prepare() is 3 stores (24 bytes) vs FxHashSet clear.
     * Bit test+set is branchless shift+mask vs hash+probe+insert.
     */
    let mut candidates = std::mem::take(&mut workspace.candidates);
    let mut seen_bits = std::mem::take(&mut workspace.seen_bits);

    for candidate in candidates.drain(..) {
        let word = (candidate.pair_id / 64) as usize;
        let bit = 1u64 << (candidate.pair_id % 64);
        if seen_bits[word] & bit != 0 {
            continue;
        }
        seen_bits[word] |= bit;

        let Some(rule) = stylesheet.rules.get(candidate.rule_index) else {
            continue;
        };
        let Rule::Style(rule) = rule else {
            continue;
        };
        let Some(selector) = rule.selectors.selectors.get(candidate.selector_index) else {
            continue;
        };
        let matched = if let Some(view) = cascade_view {
            matches_selector_with_view(dom, node, selector, view)
        } else {
            matches_selector(dom, node, selector)
        };
        if matched {
            if let Some(slot) = workspace.matched_by_rule.get_mut(candidate.rule_index) {
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

    // Return scratch buffers to workspace (retain allocations for next call)
    workspace.candidates = candidates;
    workspace.seen_bits = seen_bits;

    for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
        let Some(specificity) = workspace
            .matched_by_rule
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
/*
 * apply_declaration -- apply a single CSS declaration to the cascaded style.
 *
 * WHY: This is the innermost hot loop of the CSS cascade. Called once per
 * declaration per matching rule per node. For ChatGPT: ~20,000 calls.
 *
 * OPTIMIZATION: Uses pre-computed PropertyId (u16 enum) instead of
 * string matching. PropertyId is computed once during CSS parsing
 * (see: property_id.rs lookup_property_id). This eliminates:
 *   - to_ascii_lowercase() heap allocation per call
 *   - 30+ string comparisons per call
 * Replaced with a single match on a u16 enum discriminant.
 *
 * See: property_id.rs for the ID table
 * See: parser.rs parse_declarations() for where IDs are assigned
 */
fn apply_declaration(
    cascaded: &mut CascadedStyle,
    declaration: &Declaration,
    specificity: Specificity,
    order: usize,
) {
    use crate::property_id::PropertyId;
    match declaration.property_id {
        PropertyId::Display => {
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
        PropertyId::Color => {
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
        PropertyId::BackgroundColor => {
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
        PropertyId::FontSize => {
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
        PropertyId::LineHeight => {
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
        PropertyId::FontFamily => {
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
        PropertyId::Margin => {
            if let Some([top, right, bottom, left]) = parse_margin_edges(&declaration.value) {
                let (imp, spec, ord) = (declaration.important, specificity, order);
                apply_property(&mut cascaded.margin_top, top, imp, spec, ord);
                apply_property(&mut cascaded.margin_right, right, imp, spec, ord);
                apply_property(&mut cascaded.margin_bottom, bottom, imp, spec, ord);
                apply_property(&mut cascaded.margin_left, left, imp, spec, ord);
            }
        }
        PropertyId::MarginTop => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.margin_top,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MarginRight => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.margin_right,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MarginBottom => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.margin_bottom,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MarginLeft => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.margin_left,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Padding => {
            if let Some(value) = parse_edges(&declaration.value) {
                let (imp, spec, ord) = (declaration.important, specificity, order);
                apply_property(&mut cascaded.padding_top, value.top, imp, spec, ord);
                apply_property(&mut cascaded.padding_right, value.right, imp, spec, ord);
                apply_property(&mut cascaded.padding_bottom, value.bottom, imp, spec, ord);
                apply_property(&mut cascaded.padding_left, value.left, imp, spec, ord);
            }
        }
        PropertyId::PaddingTop => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.padding_top,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::PaddingRight => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.padding_right,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::PaddingBottom => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.padding_bottom,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::PaddingLeft => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.padding_left,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Border => {
            let (width, style, color) = parse_border_shorthand(&declaration.value);
            if let Some(w) = width {
                apply_property(
                    &mut cascaded.border,
                    Edges::all(w),
                    declaration.important,
                    specificity,
                    order,
                );
            }
            if let Some(s) = style {
                apply_property(
                    &mut cascaded.border_style,
                    s,
                    declaration.important,
                    specificity,
                    order,
                );
            }
            if let Some(c) = color {
                apply_property(
                    &mut cascaded.border_color,
                    c,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::BorderWidth => {
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
        PropertyId::FlexDirection => {
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
        PropertyId::FlexWrap => {
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
        PropertyId::FlexFlow => {
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
        PropertyId::JustifyContent => {
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
        PropertyId::AlignItems => {
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
        PropertyId::AlignSelf => {
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
        PropertyId::Gap => {
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
        PropertyId::RowGap => {
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
        PropertyId::ColumnGap => {
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
        PropertyId::FlexGrow => {
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
        PropertyId::FlexShrink => {
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
        PropertyId::FlexBasis => {
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
        PropertyId::Flex => {
            let (grow, shrink, basis) = parse_flex_shorthand(&declaration.value);
            if let Some(g) = grow {
                apply_property(
                    &mut cascaded.flex_grow,
                    g,
                    declaration.important,
                    specificity,
                    order,
                );
            }
            if let Some(s) = shrink {
                apply_property(
                    &mut cascaded.flex_shrink,
                    s,
                    declaration.important,
                    specificity,
                    order,
                );
            }
            if let Some(b) = basis {
                apply_property(
                    &mut cascaded.flex_basis,
                    b,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Order => {
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
        PropertyId::Position => {
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
        PropertyId::Top => {
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
        PropertyId::Right => {
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
        PropertyId::Bottom => {
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
        PropertyId::Left => {
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
        PropertyId::ZIndex => {
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
        PropertyId::Overflow => {
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
        PropertyId::OverflowX => {
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
        PropertyId::OverflowY => {
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
        PropertyId::Opacity => {
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
        // Visual decorations
        PropertyId::BorderRadius => {
            if let Some(value) = parse_border_radius(&declaration.value) {
                apply_property(
                    &mut cascaded.border_radius,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::BoxShadow => {
            if let Some(value) = parse_box_shadow(&declaration.value) {
                apply_property(
                    &mut cascaded.box_shadow,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::TextAlign => {
            if let Some(value) = parse_text_align(&declaration.value) {
                apply_property(
                    &mut cascaded.text_align,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::FontWeight => {
            if let Some(value) = parse_font_weight(&declaration.value) {
                apply_property(
                    &mut cascaded.font_weight,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::FontStyle => {
            if let Some(value) = parse_font_style(&declaration.value) {
                apply_property(
                    &mut cascaded.font_style,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::BackgroundImage => {
            if let Some(value) = parse_linear_gradient(&declaration.value) {
                apply_property(
                    &mut cascaded.background_image,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Sizing
        PropertyId::Width => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.width,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Height => {
            if let Some(value) = parse_length_or_auto(&declaration.value) {
                apply_property(
                    &mut cascaded.height,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MinWidth => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.min_width,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MaxWidth => {
            if let Some(value) = parse_max_dimension(&declaration.value) {
                apply_property(
                    &mut cascaded.max_width,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MinHeight => {
            if let Some(value) = parse_length(&declaration.value) {
                apply_property(
                    &mut cascaded.min_height,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::MaxHeight => {
            if let Some(value) = parse_max_dimension(&declaration.value) {
                apply_property(
                    &mut cascaded.max_height,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Border rendering
        PropertyId::BorderColor => {
            if let Some(value) = parse_color(&declaration.value) {
                apply_property(
                    &mut cascaded.border_color,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::BorderStyle => {
            if let Some(value) = parse_border_style_value(&declaration.value) {
                apply_property(
                    &mut cascaded.border_style,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        // Text / visual
        PropertyId::TextDecoration => {
            if let Some(value) = parse_text_decoration_value(&declaration.value) {
                apply_property(
                    &mut cascaded.text_decoration,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::LetterSpacing => {
            if let Some(value) = parse_spacing_px(&declaration.value) {
                apply_property(
                    &mut cascaded.letter_spacing,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::WordSpacing => {
            if let Some(value) = parse_spacing_px(&declaration.value) {
                apply_property(
                    &mut cascaded.word_spacing,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::WhiteSpace => {
            if let Some(value) = parse_white_space_value(&declaration.value) {
                apply_property(
                    &mut cascaded.white_space,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Visibility => {
            if let Some(value) = parse_visibility_value(&declaration.value) {
                apply_property(
                    &mut cascaded.visibility,
                    value,
                    declaration.important,
                    specificity,
                    order,
                );
            }
        }
        PropertyId::Unknown => {}
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
        "grid" => Some(Display::Grid),
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
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("em") => {
            value.parse::<f32>().ok().map(Length::Em)
        }
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("rem") => {
            value.parse::<f32>().ok().map(Length::Rem)
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

fn parse_border_radius(tokens: &[CssToken]) -> Option<f32> {
    tokens.iter().find_map(|token| match token {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok()
        }
        CssToken::Percentage(value) => value.parse::<f32>().ok(),
        CssToken::Number(value) if value == "0" => Some(0.0),
        _ => None,
    })
}

fn parse_text_align(tokens: &[CssToken]) -> Option<TextAlign> {
    match first_ident(tokens)? {
        "left" => Some(TextAlign::Left),
        "right" => Some(TextAlign::Right),
        "center" => Some(TextAlign::Center),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

fn parse_font_weight(tokens: &[CssToken]) -> Option<FontWeight> {
    tokens.iter().find_map(|token| match token {
        CssToken::Whitespace => None,
        CssToken::Ident(value) => match value.to_ascii_lowercase().as_str() {
            "normal" => Some(FontWeight::Normal),
            "bold" => Some(FontWeight::Bold),
            "bolder" => Some(FontWeight::Bolder),
            "lighter" => Some(FontWeight::Lighter),
            _ => None,
        },
        CssToken::Number(value) => value.parse::<u16>().ok().map(FontWeight::Number),
        _ => None,
    })
}

fn parse_font_style(tokens: &[CssToken]) -> Option<FontStyle> {
    match first_ident(tokens)? {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        "oblique" => Some(FontStyle::Oblique),
        _ => None,
    }
}

fn parse_box_shadow(tokens: &[CssToken]) -> Option<BoxShadow> {
    if first_ident(tokens) == Some("none") {
        return None;
    }
    let mut lengths = Vec::new();
    let mut color = None;
    let mut inset = false;
    for token in tokens {
        match token {
            CssToken::Ident(value) if value.eq_ignore_ascii_case("inset") => {
                inset = true;
            }
            CssToken::Ident(value) => {
                if let Some(parsed) = parse_named_color(value) {
                    color = Some(parsed);
                }
            }
            CssToken::Hash(value) => {
                if let Some(parsed) = parse_hex_color(value) {
                    color = Some(parsed);
                }
            }
            _ => {
                if let Some(length) = parse_length_token(token) {
                    lengths.push(length);
                }
            }
        }
    }
    if lengths.len() < 2 {
        return None;
    }
    let px = |l: &Length| match l {
        Length::Px(v) | Length::Percent(v) | Length::Em(v) | Length::Rem(v) => *v,
    };
    Some(BoxShadow {
        offset_x: px(&lengths[0]),
        offset_y: px(&lengths[1]),
        blur_radius: lengths.get(2).map(px).unwrap_or(0.0),
        spread_radius: lengths.get(3).map(px).unwrap_or(0.0),
        color: color.unwrap_or_else(Color::black),
        inset,
    })
}

/*
 * parse_linear_gradient -- parse background-image: linear-gradient(...).
 *
 * Supports:
 *   linear-gradient(<angle>deg, <stop>, ...)
 *   linear-gradient(to top|right|bottom|left, <stop>, ...)
 *   linear-gradient(to top|right bottom|left, <stop>, ...)   (diagonal)
 *   linear-gradient(<stop>, ...)                              (default 180deg)
 *
 * Each <stop> is: <color> [<percentage>]
 * Missing percentages are auto-distributed evenly across the range [0, 1].
 *
 * Returns None if the token list contains no linear-gradient() function,
 * if fewer than two stops are parseable, or on any structural error.
 */
fn parse_linear_gradient(tokens: &[CssToken]) -> Option<LinearGradient> {
    // Find the Function("linear-gradient") token.
    let func_pos = tokens.iter().position(
        |t| matches!(t, CssToken::Function(name) if name.eq_ignore_ascii_case("linear-gradient")),
    )?;

    // Collect inner tokens up to matching ParenClose.
    let mut inner: Vec<&CssToken> = Vec::new();
    let mut depth = 1usize;
    for token in &tokens[func_pos + 1..] {
        match token {
            CssToken::ParenOpen => {
                depth += 1;
                inner.push(token);
            }
            CssToken::ParenClose => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                inner.push(token);
            }
            _ => inner.push(token),
        }
    }

    // Split inner tokens by top-level commas into argument groups.
    let mut args: Vec<Vec<&CssToken>> = Vec::new();
    let mut current: Vec<&CssToken> = Vec::new();
    for token in &inner {
        if matches!(token, CssToken::Comma) {
            args.push(std::mem::take(&mut current));
        } else {
            current.push(token);
        }
    }
    args.push(current);

    if args.is_empty() {
        return None;
    }

    // Try to parse first arg as an angle expression.
    let first_nowhite: Vec<&CssToken> = args[0]
        .iter()
        .copied()
        .filter(|t| !matches!(t, CssToken::Whitespace))
        .collect();
    let (angle_deg, stop_start) = match gradient_angle(&first_nowhite) {
        Some(a) => (a, 1),
        None => (180.0, 0), // default: to bottom
    };

    // Parse remaining args as color stops.
    let color_args = &args[stop_start..];
    let count = color_args.len();
    if count < 2 {
        return None;
    }

    let mut stops: Vec<(f32, Color)> = Vec::with_capacity(count);
    for (i, arg) in color_args.iter().enumerate() {
        let arg_nowhite: Vec<&CssToken> = arg
            .iter()
            .copied()
            .filter(|t| !matches!(t, CssToken::Whitespace))
            .collect();
        let auto_pos = i as f32 / (count - 1) as f32;
        if let Some(stop) = gradient_color_stop(&arg_nowhite, auto_pos) {
            stops.push(stop);
        }
    }

    if stops.len() < 2 {
        return None;
    }
    Some(LinearGradient { angle_deg, stops })
}

/*
 * gradient_angle -- parse the optional angle/direction first argument.
 *
 * Returns the angle in degrees (CSS convention: 0=to-top, 90=to-right,
 * 180=to-bottom, 270=to-left), or None if tokens do not look like an angle.
 */
fn gradient_angle(tokens: &[&CssToken]) -> Option<f32> {
    match tokens {
        [CssToken::Dimension { value, unit }] if unit.eq_ignore_ascii_case("deg") => {
            value.parse().ok()
        }
        [CssToken::Dimension { value, unit }] if unit.eq_ignore_ascii_case("grad") => {
            value.parse::<f32>().ok().map(|g| g * 0.9)
        }
        [CssToken::Dimension { value, unit }] if unit.eq_ignore_ascii_case("rad") => {
            value.parse::<f32>().ok().map(|r| r.to_degrees())
        }
        [CssToken::Dimension { value, unit }] if unit.eq_ignore_ascii_case("turn") => {
            value.parse::<f32>().ok().map(|t| t * 360.0)
        }
        [CssToken::Ident(to), CssToken::Ident(dir)] if to.eq_ignore_ascii_case("to") => {
            match dir.as_str() {
                d if d.eq_ignore_ascii_case("top") => Some(0.0),
                d if d.eq_ignore_ascii_case("right") => Some(90.0),
                d if d.eq_ignore_ascii_case("bottom") => Some(180.0),
                d if d.eq_ignore_ascii_case("left") => Some(270.0),
                _ => None,
            }
        }
        [CssToken::Ident(to), CssToken::Ident(v), CssToken::Ident(h)]
            if to.eq_ignore_ascii_case("to") =>
        {
            // Diagonal directions.
            let is_top = v.eq_ignore_ascii_case("top") || h.eq_ignore_ascii_case("top");
            let is_right = v.eq_ignore_ascii_case("right") || h.eq_ignore_ascii_case("right");
            let is_bottom = v.eq_ignore_ascii_case("bottom") || h.eq_ignore_ascii_case("bottom");
            let is_left = v.eq_ignore_ascii_case("left") || h.eq_ignore_ascii_case("left");
            match (is_top, is_right, is_bottom, is_left) {
                (true, true, false, false) => Some(45.0),
                (false, true, true, false) => Some(135.0),
                (false, false, true, true) => Some(225.0),
                (true, false, false, true) => Some(315.0),
                _ => None,
            }
        }
        _ => None,
    }
}

/*
 * gradient_color_stop -- parse one color stop from an arg group.
 *
 * tokens: whitespace-stripped tokens for one comma-delimited arg.
 * auto_pos: position to use if none is specified (evenly distributed).
 *
 * Returns (position [0,1], color) or None if no color is parseable.
 */
fn gradient_color_stop(tokens: &[&CssToken], auto_pos: f32) -> Option<(f32, Color)> {
    let color = match tokens.first()? {
        CssToken::Ident(name) => parse_named_color(name)?,
        CssToken::Hash(value) => parse_hex_color(value)?,
        _ => return None,
    };
    let pos = match tokens.get(1) {
        Some(CssToken::Percentage(v)) => v.parse::<f32>().ok().map(|p| p / 100.0)?,
        _ => auto_pos,
    };
    Some((pos, color))
}

fn parse_flex_shorthand(tokens: &[CssToken]) -> (Option<f32>, Option<f32>, Option<FlexBasis>) {
    // CSS flex shorthand per spec:
    //   none          -> 0 0 auto
    //   auto          -> 1 1 auto
    //   <n>           -> <n> 1 0  (basis=0 when omitted from shorthand)
    //   <n> <n>       -> grow shrink 0
    //   <n> <basis>   -> grow 1 basis
    //   <n> <n> <basis> -> all three
    let ident = first_ident(tokens);
    if ident == Some("none") {
        return (Some(0.0), Some(0.0), Some(FlexBasis::Auto));
    }
    if ident == Some("auto") {
        return (Some(1.0), Some(1.0), Some(FlexBasis::Auto));
    }

    let mut numbers: Vec<f32> = Vec::new();
    let mut basis: Option<FlexBasis> = None;
    for token in tokens {
        match token {
            CssToken::Whitespace => continue,
            CssToken::Number(v) => {
                if let Ok(n) = v.parse::<f32>() {
                    numbers.push(n);
                }
            }
            CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
                if let Ok(px) = value.parse::<f32>() {
                    basis = Some(FlexBasis::Length(Length::Px(px)));
                }
            }
            CssToken::Percentage(value) => {
                if let Ok(pct) = value.parse::<f32>() {
                    basis = Some(FlexBasis::Length(Length::Percent(pct)));
                }
            }
            CssToken::Ident(v) if v.eq_ignore_ascii_case("auto") && !numbers.is_empty() => {
                basis = Some(FlexBasis::Auto);
            }
            _ => {}
        }
    }

    match (numbers.len(), basis) {
        (0, _) => (None, None, None),
        (1, Some(b)) => (Some(numbers[0]), Some(1.0), Some(b)),
        (1, None) => (
            Some(numbers[0]),
            Some(1.0),
            Some(FlexBasis::Length(Length::Px(0.0))),
        ),
        (2, Some(b)) => (Some(numbers[0]), Some(numbers[1]), Some(b)),
        (2, None) => (
            Some(numbers[0]),
            Some(numbers[1]),
            Some(FlexBasis::Length(Length::Px(0.0))),
        ),
        _ => (
            Some(numbers[0]),
            Some(numbers[1]),
            basis.or(Some(FlexBasis::Length(Length::Px(0.0)))),
        ),
    }
}

fn parse_border_shorthand(
    tokens: &[CssToken],
) -> (Option<Length>, Option<BorderStyle>, Option<Color>) {
    let mut width: Option<Length> = None;
    let mut style: Option<BorderStyle> = None;
    let mut color: Option<Color> = None;
    for token in tokens {
        match token {
            CssToken::Whitespace => continue,
            CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
                if width.is_none() {
                    width = value.parse::<f32>().ok().map(Length::Px);
                }
            }
            CssToken::Number(value) if value == "0" => {
                if width.is_none() {
                    width = Some(Length::Px(0.0));
                }
            }
            CssToken::Hash(hex) => {
                if color.is_none() {
                    color = parse_hex_color(hex);
                }
            }
            CssToken::Ident(value) => {
                let lower = value.to_ascii_lowercase();
                if style.is_none() {
                    style = match lower.as_str() {
                        "solid" => Some(BorderStyle::Solid),
                        "dashed" => Some(BorderStyle::Dashed),
                        "dotted" => Some(BorderStyle::Dotted),
                        "double" => Some(BorderStyle::Double),
                        "none" => Some(BorderStyle::None),
                        _ => None,
                    };
                    if style.is_some() {
                        continue;
                    }
                }
                if color.is_none() {
                    color = parse_named_color(&lower);
                }
            }
            _ => {}
        }
    }
    (width, style, color)
}

fn parse_max_dimension(tokens: &[CssToken]) -> Option<Option<Length>> {
    if first_ident(tokens) == Some("none") {
        return Some(None);
    }
    parse_length(tokens).map(Some)
}

fn parse_border_style_value(tokens: &[CssToken]) -> Option<BorderStyle> {
    match first_ident(tokens)? {
        "none" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        "double" => Some(BorderStyle::Double),
        _ => None,
    }
}

fn parse_text_decoration_value(tokens: &[CssToken]) -> Option<TextDecoration> {
    match first_ident(tokens)? {
        "none" => Some(TextDecoration::None),
        "underline" => Some(TextDecoration::Underline),
        "overline" => Some(TextDecoration::Overline),
        "line-through" => Some(TextDecoration::LineThrough),
        _ => None,
    }
}

fn parse_spacing_px(tokens: &[CssToken]) -> Option<f32> {
    if first_ident(tokens) == Some("normal") {
        return Some(0.0);
    }
    tokens.iter().find_map(|token| match token {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok()
        }
        CssToken::Number(value) if value == "0" => Some(0.0),
        _ => None,
    })
}

fn parse_white_space_value(tokens: &[CssToken]) -> Option<WhiteSpace> {
    match first_ident(tokens)? {
        "normal" => Some(WhiteSpace::Normal),
        "nowrap" => Some(WhiteSpace::Nowrap),
        "pre" => Some(WhiteSpace::Pre),
        "pre-wrap" => Some(WhiteSpace::PreWrap),
        "pre-line" => Some(WhiteSpace::PreLine),
        _ => None,
    }
}

fn parse_visibility_value(tokens: &[CssToken]) -> Option<Visibility> {
    match first_ident(tokens)? {
        "visible" => Some(Visibility::Visible),
        "hidden" => Some(Visibility::Hidden),
        "collapse" => Some(Visibility::Collapse),
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

fn parse_margin_value_list(tokens: &[CssToken]) -> Vec<LengthOrAuto> {
    let mut values = Vec::new();
    for token in tokens {
        if let CssToken::Ident(ident) = token {
            if ident.eq_ignore_ascii_case("auto") {
                values.push(LengthOrAuto::Auto);
                continue;
            }
        }
        if let Some(length) = parse_length_token(token) {
            values.push(LengthOrAuto::Length(length));
        }
    }
    values
}

fn parse_margin_edges(tokens: &[CssToken]) -> Option<[LengthOrAuto; 4]> {
    let values = parse_margin_value_list(tokens);
    match values.len() {
        1 => Some([values[0]; 4]),
        2 => Some([values[0], values[1], values[0], values[1]]),
        3 => Some([values[0], values[1], values[2], values[1]]),
        4 => Some([values[0], values[1], values[2], values[3]]),
        _ => None,
    }
}

/*
 * parse_font_family -- parse CSS font-family declaration tokens.
 *
 * WHY SmallVec<[SmolStr; 2]>: Typical font stacks have 1-2 entries
 * ("Helvetica", "sans-serif"). SmallVec inlines up to 2 on the stack.
 * SmolStr inlines strings <=23 bytes (all common font names) -- zero
 * heap allocation for the common case of 1-2 short font names.
 *
 * This replaces Vec<String> which allocated 1 Vec + N Strings per call.
 * For 61 nodes x 1 font-family each = 61 saved Vec+String allocations.
 */
fn parse_font_family(tokens: &[CssToken]) -> Option<SmallVec<[SmolStr; 2]>> {
    let mut families = SmallVec::<[SmolStr; 2]>::new();
    let mut current = Vec::<&str>::new();
    for token in tokens {
        match token {
            CssToken::Ident(value) => {
                current.push(value.as_str());
            }
            CssToken::String(value) => {
                current.push(value.as_str());
            }
            CssToken::Comma => {
                if !current.is_empty() {
                    families.push(SmolStr::new(current.join(" ")));
                    current.clear();
                }
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        families.push(SmolStr::new(current.join(" ")));
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

fn named_color(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color { r, g, b, a: 255 })
}

fn parse_named_color(value: &str) -> Option<Color> {
    match value.to_ascii_lowercase().as_str() {
        // CSS Color Level 4 named colors (148 total) + transparent
        "aliceblue" => named_color(240, 248, 255),
        "antiquewhite" => named_color(250, 235, 215),
        "aqua" | "cyan" => named_color(0, 255, 255),
        "aquamarine" => named_color(127, 255, 212),
        "azure" => named_color(240, 255, 255),
        "beige" => named_color(245, 245, 220),
        "bisque" => named_color(255, 228, 196),
        "black" => named_color(0, 0, 0),
        "blanchedalmond" => named_color(255, 235, 205),
        "blue" => named_color(0, 0, 255),
        "blueviolet" => named_color(138, 43, 226),
        "brown" => named_color(165, 42, 42),
        "burlywood" => named_color(222, 184, 135),
        "cadetblue" => named_color(95, 158, 160),
        "chartreuse" => named_color(127, 255, 0),
        "chocolate" => named_color(210, 105, 30),
        "coral" => named_color(255, 127, 80),
        "cornflowerblue" => named_color(100, 149, 237),
        "cornsilk" => named_color(255, 248, 220),
        "crimson" => named_color(220, 20, 60),
        "darkblue" => named_color(0, 0, 139),
        "darkcyan" => named_color(0, 139, 139),
        "darkgoldenrod" => named_color(184, 134, 11),
        "darkgray" | "darkgrey" => named_color(169, 169, 169),
        "darkgreen" => named_color(0, 100, 0),
        "darkkhaki" => named_color(189, 183, 107),
        "darkmagenta" => named_color(139, 0, 139),
        "darkolivegreen" => named_color(85, 107, 47),
        "darkorange" => named_color(255, 140, 0),
        "darkorchid" => named_color(153, 50, 204),
        "darkred" => named_color(139, 0, 0),
        "darksalmon" => named_color(233, 150, 122),
        "darkseagreen" => named_color(143, 188, 143),
        "darkslateblue" => named_color(72, 61, 139),
        "darkslategray" | "darkslategrey" => named_color(47, 79, 79),
        "darkturquoise" => named_color(0, 206, 209),
        "darkviolet" => named_color(148, 0, 211),
        "deeppink" => named_color(255, 20, 147),
        "deepskyblue" => named_color(0, 191, 255),
        "dimgray" | "dimgrey" => named_color(105, 105, 105),
        "dodgerblue" => named_color(30, 144, 255),
        "firebrick" => named_color(178, 34, 34),
        "floralwhite" => named_color(255, 250, 240),
        "forestgreen" => named_color(34, 139, 34),
        "fuchsia" | "magenta" => named_color(255, 0, 255),
        "gainsboro" => named_color(220, 220, 220),
        "ghostwhite" => named_color(248, 248, 255),
        "gold" => named_color(255, 215, 0),
        "goldenrod" => named_color(218, 165, 32),
        "gray" | "grey" => named_color(128, 128, 128),
        "green" => named_color(0, 128, 0),
        "greenyellow" => named_color(173, 255, 47),
        "honeydew" => named_color(240, 255, 240),
        "hotpink" => named_color(255, 105, 180),
        "indianred" => named_color(205, 92, 92),
        "indigo" => named_color(75, 0, 130),
        "ivory" => named_color(255, 255, 240),
        "khaki" => named_color(240, 230, 140),
        "lavender" => named_color(230, 230, 250),
        "lavenderblush" => named_color(255, 240, 245),
        "lawngreen" => named_color(124, 252, 0),
        "lemonchiffon" => named_color(255, 250, 205),
        "lightblue" => named_color(173, 216, 230),
        "lightcoral" => named_color(240, 128, 128),
        "lightcyan" => named_color(224, 255, 255),
        "lightgoldenrodyellow" => named_color(250, 250, 210),
        "lightgray" | "lightgrey" => named_color(211, 211, 211),
        "lightgreen" => named_color(144, 238, 144),
        "lightpink" => named_color(255, 182, 193),
        "lightsalmon" => named_color(255, 160, 122),
        "lightseagreen" => named_color(32, 178, 170),
        "lightskyblue" => named_color(135, 206, 250),
        "lightslategray" | "lightslategrey" => named_color(119, 136, 153),
        "lightsteelblue" => named_color(176, 196, 222),
        "lightyellow" => named_color(255, 255, 224),
        "lime" => named_color(0, 255, 0),
        "limegreen" => named_color(50, 205, 50),
        "linen" => named_color(250, 240, 230),
        "maroon" => named_color(128, 0, 0),
        "mediumaquamarine" => named_color(102, 205, 170),
        "mediumblue" => named_color(0, 0, 205),
        "mediumorchid" => named_color(186, 85, 211),
        "mediumpurple" => named_color(147, 112, 219),
        "mediumseagreen" => named_color(60, 179, 113),
        "mediumslateblue" => named_color(123, 104, 238),
        "mediumspringgreen" => named_color(0, 250, 154),
        "mediumturquoise" => named_color(72, 209, 204),
        "mediumvioletred" => named_color(199, 21, 133),
        "midnightblue" => named_color(25, 25, 112),
        "mintcream" => named_color(245, 255, 250),
        "mistyrose" => named_color(255, 228, 225),
        "moccasin" => named_color(255, 228, 181),
        "navajowhite" => named_color(255, 222, 173),
        "navy" => named_color(0, 0, 128),
        "oldlace" => named_color(253, 245, 230),
        "olive" => named_color(128, 128, 0),
        "olivedrab" => named_color(107, 142, 35),
        "orange" => named_color(255, 165, 0),
        "orangered" => named_color(255, 69, 0),
        "orchid" => named_color(218, 112, 214),
        "palegoldenrod" => named_color(238, 232, 170),
        "palegreen" => named_color(152, 251, 152),
        "paleturquoise" => named_color(175, 238, 238),
        "palevioletred" => named_color(219, 112, 147),
        "papayawhip" => named_color(255, 239, 213),
        "peachpuff" => named_color(255, 218, 185),
        "peru" => named_color(205, 133, 63),
        "pink" => named_color(255, 192, 203),
        "plum" => named_color(221, 160, 221),
        "powderblue" => named_color(176, 224, 230),
        "purple" => named_color(128, 0, 128),
        "rebeccapurple" => named_color(102, 51, 153),
        "red" => named_color(255, 0, 0),
        "rosybrown" => named_color(188, 143, 143),
        "royalblue" => named_color(65, 105, 225),
        "saddlebrown" => named_color(139, 69, 19),
        "salmon" => named_color(250, 128, 114),
        "sandybrown" => named_color(244, 164, 96),
        "seagreen" => named_color(46, 139, 87),
        "seashell" => named_color(255, 245, 238),
        "sienna" => named_color(160, 82, 45),
        "silver" => named_color(192, 192, 192),
        "skyblue" => named_color(135, 206, 235),
        "slateblue" => named_color(106, 90, 205),
        "slategray" | "slategrey" => named_color(112, 128, 144),
        "snow" => named_color(255, 250, 250),
        "springgreen" => named_color(0, 255, 127),
        "steelblue" => named_color(70, 130, 180),
        "tan" => named_color(210, 180, 140),
        "teal" => named_color(0, 128, 128),
        "thistle" => named_color(216, 191, 216),
        "tomato" => named_color(255, 99, 71),
        "transparent" => Some(Color::transparent()),
        "turquoise" => named_color(64, 224, 208),
        "violet" => named_color(238, 130, 238),
        "wheat" => named_color(245, 222, 179),
        "white" => named_color(255, 255, 255),
        "whitesmoke" => named_color(245, 245, 245),
        "yellow" => named_color(255, 255, 0),
        "yellowgreen" => named_color(154, 205, 50),
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

#[cfg(test)]
mod tests {
    use crate::parser::parse_stylesheet;
    use crate::style::LinearGradient;

    fn gradient_from_rule(css: &str) -> Option<LinearGradient> {
        let sheet = parse_stylesheet(css).ok()?;
        let rule = sheet.rules.first()?;
        let crate::parser::Rule::Style(sr) = rule else {
            return None;
        };
        let decl = sr
            .declarations
            .iter()
            .find(|d| d.property_id == crate::property_id::PropertyId::BackgroundImage)?;
        super::parse_linear_gradient(&decl.value)
    }

    #[test]
    fn test_angle_deg() {
        let g = gradient_from_rule("a { background-image: linear-gradient(90deg, red, blue); }")
            .expect("should parse");
        assert!((g.angle_deg - 90.0).abs() < 0.01);
        assert_eq!(g.stops.len(), 2);
        assert!((g.stops[0].0 - 0.0).abs() < 0.01);
        assert!((g.stops[1].0 - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_to_right() {
        let g = gradient_from_rule(
            "a { background-image: linear-gradient(to right, #ff0000, #0000ff); }",
        )
        .expect("should parse");
        assert!((g.angle_deg - 90.0).abs() < 0.01);
        assert_eq!(g.stops.len(), 2);
    }

    #[test]
    fn test_default_direction() {
        let g = gradient_from_rule("a { background-image: linear-gradient(red, blue); }")
            .expect("should parse");
        assert!((g.angle_deg - 180.0).abs() < 0.01);
    }

    #[test]
    fn test_explicit_positions() {
        let g =
            gradient_from_rule("a { background-image: linear-gradient(0deg, red 0%, blue 100%); }")
                .expect("should parse");
        assert!((g.stops[0].0 - 0.0).abs() < 0.01);
        assert!((g.stops[1].0 - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_too_few_stops_rejected() {
        let result = gradient_from_rule("a { background-image: linear-gradient(90deg, red); }");
        assert!(result.is_none(), "single stop should be rejected");
    }
}
