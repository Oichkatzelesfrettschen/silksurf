//! Painting an inline `<svg>` subtree.
//!
//! usvg reads SVG source text rather than a DOM, so the subtree the HTML
//! parser built is written back out and handed to it. Serializing is
//! tractable here in a way it is not for HTML: foreign content has no void
//! elements, no implied tags, and no raw-text elements, so every node writes
//! as its own start tag, its children, and its end tag.
//!
//! The rasterized result is an `ImageSurface`, which is what lets an `<svg>`
//! reach the frame through `DisplayItem::Image` alongside `<img>` rather than
//! through a display item, a scalar arm, and an ARGB arm of its own.

use crate::ImageSurface;
use silksurf_dom::{Dom, NodeId, NodeKind};
use std::sync::Arc;

/// The namespace an SVG root declares so usvg reads the subtree as SVG.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Writes an `<svg>` subtree back to SVG source text.
///
/// The root gains an `xmlns` when it carries none, because the HTML parser
/// records the namespace on the node rather than as an attribute and usvg
/// rejects a document without it.
#[must_use]
pub fn serialize_svg(dom: &Dom, root: NodeId) -> Option<String> {
    if dom.element_name(root).ok().flatten()? != "svg" {
        return None;
    }
    let mut out = String::new();
    write_node(dom, root, true, &mut out);
    Some(out)
}

/// Writes one node and its subtree.
fn write_node(dom: &Dom, node: NodeId, is_root: bool, out: &mut String) {
    let Ok(inner) = dom.node(node) else {
        return;
    };
    match inner.kind() {
        NodeKind::Text { text } => write_escaped(text, false, out),
        NodeKind::Element {
            name, attributes, ..
        } => {
            let name = name.as_str();
            out.push('<');
            out.push_str(name);
            let mut declares_namespace = false;
            for attribute in attributes {
                let attribute_name = attribute.name.as_str();
                declares_namespace |= attribute_name == "xmlns";
                out.push(' ');
                out.push_str(attribute_name);
                out.push_str("=\"");
                write_escaped(attribute.value.as_str(), true, out);
                out.push('"');
            }
            if is_root && !declares_namespace {
                out.push_str(" xmlns=\"");
                out.push_str(SVG_NAMESPACE);
                out.push('"');
            }
            out.push('>');
            if let Ok(children) = dom.children(node) {
                for child in children.iter().copied() {
                    write_node(dom, child, false, out);
                }
            }
            out.push_str("</");
            out.push_str(name);
            out.push('>');
        }
        _ => {}
    }
}

/// Escapes the characters XML gives syntactic meaning to.
///
/// An attribute value additionally escapes the quote that delimits it; text
/// content does not, because no quote closes anything there.
fn write_escaped(text: &str, in_attribute: bool, out: &mut String) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if in_attribute => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

/// Rasterizes SVG source into an opaque-capable premultiplied RGBA surface.
///
/// The tree maps its own `viewBox` onto the requested pixel size, which is
/// what scales one icon definition to whatever box layout gave it. A source
/// usvg rejects yields nothing rather than a blank surface, so the caller
/// paints what it already had.
#[must_use]
pub fn rasterize_svg(source: &str, width: u32, height: u32) -> Option<ImageSurface> {
    if width == 0 || height == 0 {
        return None;
    }
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_str(source, &options).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(ImageSurface {
        width,
        height,
        rgba: Arc::from(pixmap.take().into_boxed_slice()),
    })
}

/// The intrinsic pixel size an `<svg>` element reports to layout.
///
/// CSS Images 3 takes the `width` and `height` presentation attributes when
/// present, and falls back to the `viewBox` extent, which is the only
/// intrinsic size an icon defined purely in user units carries.
#[must_use]
pub fn svg_intrinsic_size(dom: &Dom, node: NodeId) -> Option<(f32, f32)> {
    let attribute = |name: &str| -> Option<String> {
        dom.attributes(node)
            .ok()?
            .iter()
            .find(|attr| attr.name.as_str() == name)
            .map(|attr| attr.value.as_str().to_string())
    };
    let length = |raw: Option<String>| -> Option<f32> {
        let raw = raw?;
        let digits = raw.trim().trim_end_matches("px");
        digits.parse::<f32>().ok().filter(|value| *value > 0.0)
    };
    if let (Some(width), Some(height)) = (length(attribute("width")), length(attribute("height"))) {
        return Some((width, height));
    }
    let view_box = attribute("viewBox")?;
    let extents: Vec<f32> = view_box
        .split([' ', ','])
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<f32>().ok())
        .collect();
    let [_, _, width, height] = extents[..] else {
        return None;
    };
    (width > 0.0 && height > 0.0).then_some((width, height))
}
