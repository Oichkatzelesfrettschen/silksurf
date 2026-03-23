//! DOM data structures and traversal APIs (cleanroom).

pub mod diff;

use silksurf_core::{Atom, SilkInterner, SmallString, should_intern_identifier};
use smallvec::SmallVec;
use std::sync::RwLock;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct NodeId(usize);

impl NodeId {
    /// Create a NodeId from a raw index. Use only for testing or FFI.
    pub fn from_raw(index: usize) -> Self {
        NodeId(index)
    }

    /// Get the raw index.
    pub fn raw(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attribute {
    pub name: AttributeName,
    pub value: SmallString,
    pub value_atom: Option<Atom>,
    pub value_atoms: SmallVec<[Atom; 4]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TagName {
    Html,
    Head,
    Body,
    Title,
    Meta,
    Link,
    Script,
    Style,
    Div,
    Span,
    P,
    A,
    Img,
    Table,
    Thead,
    Tbody,
    Tfoot,
    Tr,
    Th,
    Td,
    Ul,
    Ol,
    Li,
    Form,
    Input,
    Button,
    Textarea,
    Select,
    Option,
    Header,
    Footer,
    Section,
    Article,
    Nav,
    Main,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Br,
    Hr,
    Pre,
    Code,
    Small,
    Strong,
    Em,
    B,
    I,
    Custom(SmallString),
}

impl TagName {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "html" => TagName::Html,
            "head" => TagName::Head,
            "body" => TagName::Body,
            "title" => TagName::Title,
            "meta" => TagName::Meta,
            "link" => TagName::Link,
            "script" => TagName::Script,
            "style" => TagName::Style,
            "div" => TagName::Div,
            "span" => TagName::Span,
            "p" => TagName::P,
            "a" => TagName::A,
            "img" => TagName::Img,
            "table" => TagName::Table,
            "thead" => TagName::Thead,
            "tbody" => TagName::Tbody,
            "tfoot" => TagName::Tfoot,
            "tr" => TagName::Tr,
            "th" => TagName::Th,
            "td" => TagName::Td,
            "ul" => TagName::Ul,
            "ol" => TagName::Ol,
            "li" => TagName::Li,
            "form" => TagName::Form,
            "input" => TagName::Input,
            "button" => TagName::Button,
            "textarea" => TagName::Textarea,
            "select" => TagName::Select,
            "option" => TagName::Option,
            "header" => TagName::Header,
            "footer" => TagName::Footer,
            "section" => TagName::Section,
            "article" => TagName::Article,
            "nav" => TagName::Nav,
            "main" => TagName::Main,
            "h1" => TagName::H1,
            "h2" => TagName::H2,
            "h3" => TagName::H3,
            "h4" => TagName::H4,
            "h5" => TagName::H5,
            "h6" => TagName::H6,
            "br" => TagName::Br,
            "hr" => TagName::Hr,
            "pre" => TagName::Pre,
            "code" => TagName::Code,
            "small" => TagName::Small,
            "strong" => TagName::Strong,
            "em" => TagName::Em,
            "b" => TagName::B,
            "i" => TagName::I,
            _ => TagName::Custom(SmallString::from(lower)),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            TagName::Html => "html",
            TagName::Head => "head",
            TagName::Body => "body",
            TagName::Title => "title",
            TagName::Meta => "meta",
            TagName::Link => "link",
            TagName::Script => "script",
            TagName::Style => "style",
            TagName::Div => "div",
            TagName::Span => "span",
            TagName::P => "p",
            TagName::A => "a",
            TagName::Img => "img",
            TagName::Table => "table",
            TagName::Thead => "thead",
            TagName::Tbody => "tbody",
            TagName::Tfoot => "tfoot",
            TagName::Tr => "tr",
            TagName::Th => "th",
            TagName::Td => "td",
            TagName::Ul => "ul",
            TagName::Ol => "ol",
            TagName::Li => "li",
            TagName::Form => "form",
            TagName::Input => "input",
            TagName::Button => "button",
            TagName::Textarea => "textarea",
            TagName::Select => "select",
            TagName::Option => "option",
            TagName::Header => "header",
            TagName::Footer => "footer",
            TagName::Section => "section",
            TagName::Article => "article",
            TagName::Nav => "nav",
            TagName::Main => "main",
            TagName::H1 => "h1",
            TagName::H2 => "h2",
            TagName::H3 => "h3",
            TagName::H4 => "h4",
            TagName::H5 => "h5",
            TagName::H6 => "h6",
            TagName::Br => "br",
            TagName::Hr => "hr",
            TagName::Pre => "pre",
            TagName::Code => "code",
            TagName::Small => "small",
            TagName::Strong => "strong",
            TagName::Em => "em",
            TagName::B => "b",
            TagName::I => "i",
            TagName::Custom(name) => name.as_str(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AttributeName {
    Id,
    Class,
    Href,
    Src,
    Type,
    Rel,
    Title,
    Name,
    Lang,
    Alt,
    Style,
    Custom(SmallString),
}

impl AttributeName {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "id" => AttributeName::Id,
            "class" => AttributeName::Class,
            "href" => AttributeName::Href,
            "src" => AttributeName::Src,
            "type" => AttributeName::Type,
            "rel" => AttributeName::Rel,
            "title" => AttributeName::Title,
            "name" => AttributeName::Name,
            "lang" => AttributeName::Lang,
            "alt" => AttributeName::Alt,
            "style" => AttributeName::Style,
            _ => AttributeName::Custom(SmallString::from(lower)),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            AttributeName::Id => "id",
            AttributeName::Class => "class",
            AttributeName::Href => "href",
            AttributeName::Src => "src",
            AttributeName::Type => "type",
            AttributeName::Rel => "rel",
            AttributeName::Title => "title",
            AttributeName::Name => "name",
            AttributeName::Lang => "lang",
            AttributeName::Alt => "alt",
            AttributeName::Style => "style",
            AttributeName::Custom(name) => name.as_str(),
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        self.as_str().eq_ignore_ascii_case(name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Document,
    Doctype {
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
    },
    Element {
        name: TagName,
        namespace: Namespace,
        attributes: Vec<Attribute>,
    },
    Text {
        text: String,
    },
    Comment {
        data: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    children: SmallVec<[NodeId; 8]>,
}

#[derive(Default)]
pub struct Dom {
    nodes: Vec<Node>,
    interner: RwLock<SilkInterner>,
    dirty_nodes: Vec<NodeId>,
    dirty_batch: Vec<NodeId>,
    batch_depth: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DomError {
    UnknownNode(NodeId),
    AlreadyHasParent(NodeId),
    NotElement(NodeId),
    NotText(NodeId),
}

impl Dom {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            interner: RwLock::new(SilkInterner::new()),
            dirty_nodes: Vec::new(),
            dirty_batch: Vec::new(),
            batch_depth: 0,
        }
    }

    pub fn create_document(&mut self) -> NodeId {
        self.push_node(NodeKind::Document)
    }

    pub fn create_element(&mut self, name: impl Into<String>) -> NodeId {
        self.create_element_ns(name, Namespace::Html)
    }

    pub fn create_element_ns(&mut self, name: impl Into<String>, namespace: Namespace) -> NodeId {
        let name = name.into();
        let name = TagName::from_str(&name);
        self.push_node(NodeKind::Element {
            name,
            namespace,
            attributes: Vec::new(),
        })
    }

    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.push_node(NodeKind::Text { text: text.into() })
    }

    pub fn create_comment(&mut self, data: impl Into<String>) -> NodeId {
        self.push_node(NodeKind::Comment { data: data.into() })
    }

    pub fn create_doctype(
        &mut self,
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
    ) -> NodeId {
        self.push_node(NodeKind::Doctype {
            name,
            public_id,
            system_id,
        })
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_index = self.node_index(parent)?;
        let child_index = self.node_index(child)?;

        if self.nodes[child_index].parent.is_some() {
            return Err(DomError::AlreadyHasParent(child));
        }

        self.nodes[child_index].parent = Some(parent);
        self.nodes[parent_index].children.push(child);
        self.mark_dirty(parent);
        self.mark_dirty(child);
        Ok(())
    }

    /// Remove a child node from its parent.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_index = self.node_index(parent)?;
        let child_index = self.node_index(child)?;
        self.nodes[parent_index].children.retain(|id| *id != child);
        self.nodes[child_index].parent = None;
        self.mark_dirty(parent);
        Ok(())
    }

    /// Insert a new child before a reference child.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        new_child: NodeId,
        ref_child: NodeId,
    ) -> Result<(), DomError> {
        let parent_index = self.node_index(parent)?;
        let new_index = self.node_index(new_child)?;
        let _ = self.node_index(ref_child)?; // validate ref exists

        // Detach new_child from old parent if needed
        if let Some(old_parent) = self.nodes[new_index].parent {
            let old_parent_index = self.node_index(old_parent)?;
            self.nodes[old_parent_index]
                .children
                .retain(|id| *id != new_child);
        }

        self.nodes[new_index].parent = Some(parent);
        let pos = self.nodes[parent_index]
            .children
            .iter()
            .position(|id| *id == ref_child);
        match pos {
            Some(idx) => self.nodes[parent_index].children.insert(idx, new_child),
            None => self.nodes[parent_index].children.push(new_child),
        }
        self.mark_dirty(parent);
        self.mark_dirty(new_child);
        Ok(())
    }

    pub fn append_text(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
    ) -> Result<NodeId, DomError> {
        let text = text.into();
        if text.is_empty() {
            return Ok(parent);
        }
        let last = self.last_child(parent)?;
        if let Some(last) = last {
            let last_index = self.node_index(last)?;
            if let NodeKind::Text { text: existing } = &mut self.nodes[last_index].kind {
                existing.push_str(&text);
                self.mark_dirty(parent);
                self.mark_dirty(last);
                return Ok(last);
            }
        }
        let text_node = self.create_text(text);
        self.append_child(parent, text_node)?;
        self.mark_dirty(parent);
        self.mark_dirty(text_node);
        Ok(text_node)
    }

    pub fn set_attribute(
        &mut self,
        id: NodeId,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), DomError> {
        let name = name.into();
        let value = value.into();
        let attr_name = AttributeName::from_str(&name);
        let value: SmallString = value.into();
        let (value_atom, value_atoms) = match attr_name {
            AttributeName::Id => {
                let atom = if value.is_empty() || !should_intern_identifier(value.as_str()) {
                    None
                } else {
                    Some(self.interner.write().unwrap().intern(value.as_str()))
                };
                (atom, SmallVec::new())
            }
            AttributeName::Class => {
                let atoms = if value.is_empty() {
                    SmallVec::new()
                } else {
                    let mut interner = self.interner.write().unwrap();
                    value
                        .split_whitespace()
                        .filter(|part| should_intern_identifier(part))
                        .map(|part| interner.intern(part))
                        .collect()
                };
                (None, atoms)
            }
            _ => {
                let atom = if value.is_empty() || !should_intern_identifier(value.as_str()) {
                    None
                } else {
                    Some(self.interner.write().unwrap().intern(value.as_str()))
                };
                (atom, SmallVec::new())
            }
        };
        let index = self.node_index(id)?;
        match &mut self.nodes[index].kind {
            NodeKind::Element { attributes, .. } => {
                if let Some(existing) = attributes.iter_mut().find(|a| a.name == attr_name) {
                    existing.value = value;
                    existing.value_atom = value_atom;
                    existing.value_atoms = value_atoms;
                } else {
                    attributes.push(Attribute {
                        name: attr_name,
                        value,
                        value_atom,
                        value_atoms,
                    });
                }
                self.mark_dirty(id);
                Ok(())
            }
            _ => Err(DomError::NotElement(id)),
        }
    }

    pub fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        if self.batch_depth == 0 {
            self.flush_dirty_batch();
        }
        std::mem::take(&mut self.dirty_nodes)
    }

    fn mark_dirty(&mut self, id: NodeId) {
        if self.batch_depth > 0 {
            self.dirty_batch.push(id);
        } else {
            self.dirty_nodes.push(id);
        }
    }

    pub fn begin_mutation_batch(&mut self) {
        self.batch_depth += 1;
    }

    pub fn end_mutation_batch(&mut self) {
        if self.batch_depth == 0 {
            return;
        }
        self.batch_depth -= 1;
        if self.batch_depth == 0 {
            self.flush_dirty_batch();
        }
    }

    pub fn with_mutation_batch<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.begin_mutation_batch();
        let result = f(self);
        self.end_mutation_batch();
        result
    }

    fn flush_dirty_batch(&mut self) {
        if self.dirty_batch.is_empty() {
            return;
        }
        self.dirty_nodes.append(&mut self.dirty_batch);
        self.dirty_nodes.sort_unstable_by_key(|id| id.0);
        self.dirty_nodes.dedup_by_key(|id| id.0);
    }

    pub fn attributes(&self, id: NodeId) -> Result<&[Attribute], DomError> {
        let index = self.node_index(id)?;
        match &self.nodes[index].kind {
            NodeKind::Element { attributes, .. } => Ok(attributes.as_slice()),
            _ => Err(DomError::NotElement(id)),
        }
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DomError> {
        let index = self.node_index(id)?;
        Ok(&self.nodes[index])
    }

    pub fn children(&self, id: NodeId) -> Result<&[NodeId], DomError> {
        let index = self.node_index(id)?;
        Ok(&self.nodes[index].children)
    }

    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        let index = self.node_index(id)?;
        Ok(self.nodes[index].parent)
    }

    pub fn first_child(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        let index = self.node_index(id)?;
        Ok(self.nodes[index].children.first().copied())
    }

    pub fn last_child(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        let index = self.node_index(id)?;
        Ok(self.nodes[index].children.last().copied())
    }

    pub fn next_sibling(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        let parent = match self.parent(id)? {
            Some(parent) => parent,
            None => return Ok(None),
        };
        let siblings = self.children(parent)?;
        for (idx, sibling) in siblings.iter().enumerate() {
            if *sibling == id {
                return Ok(siblings.get(idx + 1).copied());
            }
        }
        Ok(None)
    }

    pub fn previous_sibling(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        let parent = match self.parent(id)? {
            Some(parent) => parent,
            None => return Ok(None),
        };
        let siblings = self.children(parent)?;
        for (idx, sibling) in siblings.iter().enumerate() {
            if *sibling == id {
                return Ok(idx
                    .checked_sub(1)
                    .and_then(|pos| siblings.get(pos))
                    .copied());
            }
        }
        Ok(None)
    }

    pub fn element_name(&self, id: NodeId) -> Result<Option<&str>, DomError> {
        let index = self.node_index(id)?;
        match &self.nodes[index].kind {
            NodeKind::Element { name, .. } => Ok(Some(name.as_str())),
            _ => Ok(None),
        }
    }

    pub fn with_interner_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut SilkInterner) -> R,
    {
        let mut interner = self.interner.write().unwrap();
        f(&mut interner)
    }

    pub fn intern(&self, value: &str) -> Atom {
        self.interner.write().unwrap().intern(value)
    }

    pub fn resolve(&self, atom: Atom) -> SmallString {
        SmallString::from(self.interner.read().unwrap().resolve(atom))
    }

    pub fn child_elements(&self, id: NodeId) -> Result<Vec<NodeId>, DomError> {
        let children = self.children(id)?;
        Ok(children
            .iter()
            .copied()
            .filter(|child| self.element_name(*child).ok().flatten().is_some())
            .collect())
    }

    fn push_node(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            kind,
            parent: None,
            children: SmallVec::new(),
        });
        id
    }

    fn node_index(&self, id: NodeId) -> Result<usize, DomError> {
        if id.0 < self.nodes.len() {
            Ok(id.0)
        } else {
            Err(DomError::UnknownNode(id))
        }
    }
}

impl Node {
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }
}
