//! HTML fragment serialization through html5ever's escaping and element rules.

use std::io;

use html5ever::serialize::{HtmlSerializer, SerializeOpts, Serializer, TraversalScope};
use html5ever::{LocalName, QualName, ns};
use silksurf_dom::{Dom, Namespace, NodeId, NodeKind};

/// Serialize an element's contents, including its owned template fragment.
pub fn serialize_fragment(dom: &Dom, root: NodeId) -> io::Result<String> {
    let parent_name = match dom.node(root).map_err(|error| dom_error(&error))?.kind() {
        NodeKind::Element {
            name, namespace, ..
        } => Some(qualified_name(name.as_str(), namespace)),
        _ => None,
    };
    let mut bytes = Vec::new();
    let options = SerializeOpts {
        traversal_scope: TraversalScope::ChildrenOnly(parent_name),
        ..SerializeOpts::default()
    };
    let mut serializer = HtmlSerializer::new(&mut bytes, options);
    let mut pending = Vec::new();
    push_children(dom, root, &mut pending)?;
    while let Some((node, closing)) = pending.pop() {
        serialize_node(dom, node, closing, &mut serializer, &mut pending)?;
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn serialize_node(
    dom: &Dom,
    node: NodeId,
    closing: bool,
    serializer: &mut impl Serializer,
    pending: &mut Vec<(NodeId, bool)>,
) -> io::Result<()> {
    match dom.node(node).map_err(|error| dom_error(&error))?.kind() {
        NodeKind::Element {
            name,
            namespace,
            attributes,
        } => {
            let void_element = matches!(namespace, Namespace::Html)
                && matches!(
                    name.as_str(),
                    "area"
                        | "base"
                        | "basefont"
                        | "bgsound"
                        | "link"
                        | "meta"
                        | "hr"
                        | "br"
                        | "img"
                        | "embed"
                        | "param"
                        | "input"
                        | "keygen"
                        | "source"
                        | "track"
                        | "wbr"
                );
            let name = qualified_name(name.as_str(), namespace);
            if closing {
                return serializer.end_elem(name);
            }
            let attributes: Vec<_> = attributes
                .iter()
                .map(|attribute| {
                    (
                        QualName::new(None, ns!(), LocalName::from(attribute.name.as_str())),
                        attribute.value.as_str(),
                    )
                })
                .collect();
            serializer.start_elem(name, attributes.iter().map(|(name, value)| (name, *value)))?;
            pending.push((node, true));
            if !void_element {
                push_children(dom, node, pending)?;
            }
        }
        NodeKind::Text { text } => serializer.write_text(text)?,
        NodeKind::Comment { data } => serializer.write_comment(data)?,
        NodeKind::Doctype { name, .. } => {
            serializer.write_doctype(name.as_deref().unwrap_or_default())?;
        }
        NodeKind::Document | NodeKind::DocumentFragment => push_children(dom, node, pending)?,
    }
    Ok(())
}

fn push_children(dom: &Dom, node: NodeId, pending: &mut Vec<(NodeId, bool)>) -> io::Result<()> {
    let container = dom.template_contents(node).unwrap_or(node);
    pending.extend(
        dom.children(container)
            .map_err(|error| dom_error(&error))?
            .iter()
            .rev()
            .map(|child| (*child, false)),
    );
    Ok(())
}

fn qualified_name(name: &str, namespace: &Namespace) -> QualName {
    let namespace = match namespace {
        Namespace::Html => ns!(html),
        Namespace::Svg => ns!(svg),
        Namespace::MathMl => ns!(mathml),
        Namespace::Other(value) => html5ever::Namespace::from(value.as_str()),
    };
    QualName::new(None, namespace, LocalName::from(name))
}

fn dom_error(error: &silksurf_dom::DomError) -> io::Error {
    io::Error::other(format!("HTML serialization: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_element_with_void_html_name_retains_children() {
        let mut dom = Dom::new();
        let fragment = dom.create_document_fragment();
        let foreign = dom.create_element_ns("br", Namespace::Svg);
        let text = dom.create_text("visible");
        dom.append_child(foreign, text).expect("foreign text");
        dom.append_child(fragment, foreign).expect("fragment child");
        assert_eq!(
            serialize_fragment(&dom, fragment).expect("serialize"),
            "<br>visible</br>"
        );
    }
}
