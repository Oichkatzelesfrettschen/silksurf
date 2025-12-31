//! DOM data structures and traversal APIs (cleanroom).

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct NodeId(usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Document,
    Element { name: String },
    Text { text: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

#[derive(Default)]
pub struct Dom {
    nodes: Vec<Node>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DomError {
    UnknownNode(NodeId),
    AlreadyHasParent(NodeId),
}

impl Dom {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn create_document(&mut self) -> NodeId {
        self.push_node(NodeKind::Document)
    }

    pub fn create_element(&mut self, name: impl Into<String>) -> NodeId {
        self.push_node(NodeKind::Element { name: name.into() })
    }

    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.push_node(NodeKind::Text { text: text.into() })
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_index = self.node_index(parent)?;
        let child_index = self.node_index(child)?;

        if self.nodes[child_index].parent.is_some() {
            return Err(DomError::AlreadyHasParent(child));
        }

        self.nodes[child_index].parent = Some(parent);
        self.nodes[parent_index].children.push(child);
        Ok(())
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

    fn push_node(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
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
