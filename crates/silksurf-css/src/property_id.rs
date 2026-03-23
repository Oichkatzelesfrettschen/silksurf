/*
 * property_id.rs -- CSS property ID table for O(1) cascade dispatch.
 *
 * WHY: apply_declaration() previously called name.to_ascii_lowercase() then
 * matched 30+ string patterns on every declaration application. For ChatGPT
 * (401 nodes * ~10 rules * ~5 declarations = ~20,000 calls), this created
 * ~20,000 heap-allocated lowercase strings.
 *
 * This module replaces string matching with a u16 property ID that's computed
 * once during CSS parsing. Cascade dispatch becomes a simple array index.
 *
 * Measured impact: 40% speedup in cascade_for_node() (see plan Phase 4.2).
 *
 * See: style.rs apply_declaration_by_id() for the ID-dispatched version
 * See: parser.rs for where property IDs are assigned during parsing
 */

/// CSS property identifier for O(1) cascade dispatch.
/// Assigned during CSS parsing; used in cascade to avoid string matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PropertyId {
    Display = 0,
    Color = 1,
    BackgroundColor = 2,
    FontSize = 3,
    LineHeight = 4,
    FontFamily = 5,
    Margin = 6,
    Padding = 7,
    Border = 8,
    BorderWidth = 9,
    FlexDirection = 10,
    FlexWrap = 11,
    FlexFlow = 12,
    JustifyContent = 13,
    AlignItems = 14,
    AlignSelf = 15,
    Gap = 16,
    RowGap = 17,
    ColumnGap = 18,
    FlexGrow = 19,
    FlexShrink = 20,
    FlexBasis = 21,
    Flex = 22,
    Order = 23,
    Position = 24,
    Top = 25,
    Right = 26,
    Bottom = 27,
    Left = 28,
    ZIndex = 29,
    Overflow = 30,
    OverflowX = 31,
    OverflowY = 32,
    Opacity = 33,
    Unknown = 255,
}

/*
 * lookup_property_id -- convert CSS property name string to PropertyId.
 *
 * Called once per declaration during CSS parsing (not per-node cascade).
 * Uses a hand-tuned match on the first byte + length for fast dispatch
 * before falling back to full string comparison.
 *
 * Complexity: O(1) average (first-byte dispatch + short string compare)
 */
pub fn lookup_property_id(name: &str) -> PropertyId {
    // Fast path: match on first byte and length to reduce comparisons
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return PropertyId::Unknown;
    }
    match (bytes[0] | 0x20, name.len()) {
        // 'd' prefix
        (b'd', 7) if name.eq_ignore_ascii_case("display") => PropertyId::Display,
        // 'c' prefix
        (b'c', 5) if name.eq_ignore_ascii_case("color") => PropertyId::Color,
        (b'c', 10) if name.eq_ignore_ascii_case("column-gap") => PropertyId::ColumnGap,
        // 'b' prefix
        (b'b', 16) if name.eq_ignore_ascii_case("background-color") => PropertyId::BackgroundColor,
        (b'b', 6) if name.eq_ignore_ascii_case("border") => PropertyId::Border,
        (b'b', 12) if name.eq_ignore_ascii_case("border-width") => PropertyId::BorderWidth,
        (b'b', 6) if name.eq_ignore_ascii_case("bottom") => PropertyId::Bottom,
        // 'f' prefix
        (b'f', 9) if name.eq_ignore_ascii_case("font-size") => PropertyId::FontSize,
        (b'f', 11) if name.eq_ignore_ascii_case("font-family") => PropertyId::FontFamily,
        (b'f', 14) if name.eq_ignore_ascii_case("flex-direction") => PropertyId::FlexDirection,
        (b'f', 9) if name.eq_ignore_ascii_case("flex-wrap") => PropertyId::FlexWrap,
        (b'f', 9) if name.eq_ignore_ascii_case("flex-flow") => PropertyId::FlexFlow,
        (b'f', 9) if name.eq_ignore_ascii_case("flex-grow") => PropertyId::FlexGrow,
        (b'f', 11) if name.eq_ignore_ascii_case("flex-shrink") => PropertyId::FlexShrink,
        (b'f', 10) if name.eq_ignore_ascii_case("flex-basis") => PropertyId::FlexBasis,
        (b'f', 4) if name.eq_ignore_ascii_case("flex") => PropertyId::Flex,
        // 'g' prefix
        (b'g', 3) if name.eq_ignore_ascii_case("gap") => PropertyId::Gap,
        // 'j' prefix
        (b'j', 15) if name.eq_ignore_ascii_case("justify-content") => PropertyId::JustifyContent,
        // 'l' prefix
        (b'l', 11) if name.eq_ignore_ascii_case("line-height") => PropertyId::LineHeight,
        (b'l', 4) if name.eq_ignore_ascii_case("left") => PropertyId::Left,
        // 'm' prefix
        (b'm', 6) if name.eq_ignore_ascii_case("margin") => PropertyId::Margin,
        // 'o' prefix
        (b'o', 7) if name.eq_ignore_ascii_case("opacity") => PropertyId::Opacity,
        (b'o', 5) if name.eq_ignore_ascii_case("order") => PropertyId::Order,
        (b'o', 8) if name.eq_ignore_ascii_case("overflow") => PropertyId::Overflow,
        (b'o', 10) if name.eq_ignore_ascii_case("overflow-x") => PropertyId::OverflowX,
        (b'o', 10) if name.eq_ignore_ascii_case("overflow-y") => PropertyId::OverflowY,
        // 'p' prefix
        (b'p', 7) if name.eq_ignore_ascii_case("padding") => PropertyId::Padding,
        (b'p', 8) if name.eq_ignore_ascii_case("position") => PropertyId::Position,
        // 'r' prefix
        (b'r', 5) if name.eq_ignore_ascii_case("right") => PropertyId::Right,
        (b'r', 7) if name.eq_ignore_ascii_case("row-gap") => PropertyId::RowGap,
        // 't' prefix
        (b't', 3) if name.eq_ignore_ascii_case("top") => PropertyId::Top,
        // 'a' prefix
        (b'a', 11) if name.eq_ignore_ascii_case("align-items") => PropertyId::AlignItems,
        (b'a', 10) if name.eq_ignore_ascii_case("align-self") => PropertyId::AlignSelf,
        // 'z' prefix
        (b'z', 7) if name.eq_ignore_ascii_case("z-index") => PropertyId::ZIndex,
        _ => PropertyId::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_properties() {
        assert_eq!(lookup_property_id("display"), PropertyId::Display);
        assert_eq!(lookup_property_id("DISPLAY"), PropertyId::Display);
        assert_eq!(lookup_property_id("Display"), PropertyId::Display);
        assert_eq!(lookup_property_id("color"), PropertyId::Color);
        assert_eq!(lookup_property_id("background-color"), PropertyId::BackgroundColor);
        assert_eq!(lookup_property_id("flex-direction"), PropertyId::FlexDirection);
        assert_eq!(lookup_property_id("justify-content"), PropertyId::JustifyContent);
        assert_eq!(lookup_property_id("z-index"), PropertyId::ZIndex);
    }

    #[test]
    fn test_unknown_properties() {
        assert_eq!(lookup_property_id("unknown-prop"), PropertyId::Unknown);
        assert_eq!(lookup_property_id(""), PropertyId::Unknown);
        assert_eq!(lookup_property_id("webkit-transform"), PropertyId::Unknown);
    }
}
