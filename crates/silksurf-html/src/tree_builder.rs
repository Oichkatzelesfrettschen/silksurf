use silksurf_dom::{Dom, DomError, NodeId, NodeKind};

use crate::Token;

pub struct TreeBuilder {
    dom: Dom,
    document: NodeId,
    open_elements: Vec<NodeId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeBuildError {
    Dom(DomError),
    UnexpectedEndTag(String),
}

impl TreeBuilder {
    pub fn new() -> Self {
        let mut dom = Dom::new();
        let document = dom.create_document();
        Self {
            dom,
            document,
            open_elements: vec![document],
        }
    }

    pub fn document_id(&self) -> NodeId {
        self.document
    }

    pub fn dom(&self) -> &Dom {
        &self.dom
    }

    pub fn dom_mut(&mut self) -> &mut Dom {
        &mut self.dom
    }

    pub fn into_dom(self) -> Dom {
        self.dom
    }

    pub fn process_tokens<I>(&mut self, tokens: I) -> Result<(), TreeBuildError>
    where
        I: IntoIterator<Item = Token>,
    {
        for token in tokens {
            match token {
                Token::StartTag { name, self_closing, .. } => {
                    self.handle_start_tag(&name, self_closing)?;
                }
                Token::EndTag { name } => {
                    self.handle_end_tag(&name)?;
                }
                Token::Character { data } => {
                    self.handle_text(&data)?;
                }
                Token::Comment { .. } | Token::Doctype { .. } | Token::Eof => {
                    continue;
                }
            }
        }
        Ok(())
    }

    fn handle_start_tag(&mut self, name: &str, self_closing: bool) -> Result<(), TreeBuildError> {
        let element = self.dom.create_element(name);
        let parent = self.current_node();
        self.dom.append_child(parent, element).map_err(TreeBuildError::Dom)?;
        if !self_closing {
            self.open_elements.push(element);
        }
        Ok(())
    }

    fn handle_end_tag(&mut self, name: &str) -> Result<(), TreeBuildError> {
        let mut match_index = None;
        for idx in (0..self.open_elements.len()).rev() {
            let node_id = self.open_elements[idx];
            let node = self.dom.node(node_id).map_err(TreeBuildError::Dom)?;
            if let NodeKind::Element { name: node_name } = node.kind() {
                if node_name.eq_ignore_ascii_case(name) {
                    match_index = Some(idx);
                    break;
                }
            }
        }

        if let Some(idx) = match_index {
            self.open_elements.truncate(idx);
            Ok(())
        } else {
            Err(TreeBuildError::UnexpectedEndTag(name.to_string()))
        }
    }

    fn handle_text(&mut self, data: &str) -> Result<(), TreeBuildError> {
        if data.is_empty() {
            return Ok(());
        }
        let text = self.dom.create_text(data);
        let parent = self.current_node();
        self.dom.append_child(parent, text).map_err(TreeBuildError::Dom)?;
        Ok(())
    }

    fn current_node(&self) -> NodeId {
        *self.open_elements.last().expect("document node present")
    }
}

impl From<DomError> for TreeBuildError {
    fn from(error: DomError) -> Self {
        TreeBuildError::Dom(error)
    }
}
