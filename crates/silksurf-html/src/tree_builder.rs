use silksurf_dom::{Dom, DomError, NodeId, NodeKind};

use crate::Token;

pub struct TreeBuilder {
    dom: Dom,
    document: NodeId,
    open_elements: Vec<NodeId>,
    insertion_mode: InsertionMode,
    html_element: Option<NodeId>,
    head_element: Option<NodeId>,
    body_element: Option<NodeId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeBuildError {
    Dom(DomError),
    UnexpectedEndTag(String),
}

impl From<TreeBuildError> for silksurf_core::SilkError {
    fn from(e: TreeBuildError) -> Self {
        silksurf_core::SilkError::HtmlTreeBuild(format!("{e:?}"))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum InsertionMode {
    Initial,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
}

impl TreeBuilder {
    pub fn new() -> Self {
        let mut dom = Dom::new();
        let document = dom.create_document();
        Self {
            dom,
            document,
            open_elements: vec![document],
            insertion_mode: InsertionMode::Initial,
            html_element: None,
            head_element: None,
            body_element: None,
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

    pub fn into_dom(mut self) -> Dom {
        // Materialize the resolve table after parse completes.
        // All atoms interned during tree building become available
        // via dom.resolve_fast() without RwLock for the cascade phase.
        self.dom.materialize_resolve_table();
        self.dom
    }

    pub fn process_tokens<I>(&mut self, tokens: I) -> Result<(), TreeBuildError>
    where
        I: IntoIterator<Item = Token>,
    {
        for token in tokens {
            match token {
                Token::StartTag {
                    name,
                    attributes,
                    self_closing,
                } => {
                    self.handle_start_tag(&name, &attributes, self_closing)?;
                }
                Token::EndTag { name } => {
                    self.handle_end_tag(&name)?;
                }
                Token::Character { data } => {
                    self.handle_text(&data)?;
                }
                Token::Comment { data } => {
                    self.handle_comment(&data)?;
                }
                Token::Doctype {
                    name,
                    public_id,
                    system_id,
                    ..
                } => {
                    self.handle_doctype(name, public_id, system_id)?;
                }
                Token::Eof => continue,
            }
        }
        Ok(())
    }

    fn handle_start_tag(
        &mut self,
        name: &str,
        attributes: &[crate::Attribute],
        self_closing: bool,
    ) -> Result<(), TreeBuildError> {
        match self.insertion_mode {
            InsertionMode::Initial => {
                if name.eq_ignore_ascii_case("html") {
                    self.insert_html_element(attributes, self_closing)?;
                    self.insertion_mode = InsertionMode::BeforeHead;
                    return Ok(());
                }
                self.ensure_html_element()?;
                self.insertion_mode = InsertionMode::BeforeHead;
                self.handle_start_tag(name, attributes, self_closing)
            }
            InsertionMode::BeforeHead => {
                if name.eq_ignore_ascii_case("head") {
                    self.insert_head_element(attributes, self_closing)?;
                    self.insertion_mode = InsertionMode::InHead;
                    return Ok(());
                }
                if name.eq_ignore_ascii_case("html") {
                    return Ok(());
                }
                self.insert_head_element(&[], false)?;
                self.insertion_mode = InsertionMode::InHead;
                self.handle_start_tag(name, attributes, self_closing)
            }
            InsertionMode::InHead => {
                if name.eq_ignore_ascii_case("head") {
                    return Ok(());
                }
                if name.eq_ignore_ascii_case("body") {
                    self.pop_until_tag("head");
                    self.insertion_mode = InsertionMode::InBody;
                    return self.handle_start_tag(name, attributes, self_closing);
                }
                if is_head_element(name) {
                    let head = self.ensure_head_element()?;
                    self.insert_element(head, name, attributes, self_closing)?;
                    return Ok(());
                }
                self.pop_until_tag("head");
                self.insertion_mode = InsertionMode::AfterHead;
                self.handle_start_tag(name, attributes, self_closing)
            }
            InsertionMode::AfterHead => {
                if name.eq_ignore_ascii_case("body") {
                    self.insert_body_element(attributes, self_closing)?;
                    self.insertion_mode = InsertionMode::InBody;
                    return Ok(());
                }
                self.ensure_body_element()?;
                self.insertion_mode = InsertionMode::InBody;
                self.handle_start_tag(name, attributes, self_closing)
            }
            InsertionMode::InBody => {
                if name.eq_ignore_ascii_case("html") {
                    return Ok(());
                }
                let parent = self.current_node();
                self.insert_element(parent, name, attributes, self_closing)?;
                Ok(())
            }
        }
    }

    fn handle_end_tag(&mut self, name: &str) -> Result<(), TreeBuildError> {
        if name.eq_ignore_ascii_case("head") {
            if self.pop_until_tag("head") {
                self.insertion_mode = InsertionMode::AfterHead;
                return Ok(());
            }
        }
        if name.eq_ignore_ascii_case("body") {
            if self.pop_until_tag("body") {
                self.insertion_mode = InsertionMode::InBody;
                return Ok(());
            }
        }
        if self.pop_until_tag(name) {
            return Ok(());
        }
        Err(TreeBuildError::UnexpectedEndTag(name.to_string()))
    }

    fn handle_text(&mut self, data: &str) -> Result<(), TreeBuildError> {
        if data.is_empty() {
            return Ok(());
        }
        if self.insertion_mode != InsertionMode::InBody {
            /*
             * If the current open element is a content-bearing head
             * descendant (title, script, style, textarea, etc.), the
             * character data belongs to it -- not to an implicit body.
             *
             * The earlier behaviour unconditionally called
             * ensure_body_element() here, which had two bad effects:
             *   1. Text like "T" inside <title>T</title> was relocated
             *      into <body>, leaving <title> empty.
             *   2. The implicit <body> was pushed onto the open-elements
             *      stack while we were still nominally InHead, so a
             *      later </body> end tag found body absent from the
             *      stack and raised UnexpectedEndTag.
             *
             * Per HTML5 8.2.5.4.4 ("in head" insertion mode), character
             * data inside <title>/<script>/<style> is appended to the
             * current node verbatim. We approximate that by checking
             * whether current_node is one of doc/html/head -- the only
             * positions where stray text really requires foster-parenting
             * into body.
             */
            let current = self.current_node();
            let goes_to_current = !self.is_doc_html_or_head(current);
            if goes_to_current {
                self.dom
                    .append_text(current, data)
                    .map_err(TreeBuildError::Dom)?;
                return Ok(());
            }
            if data.trim().is_empty() {
                return Ok(());
            }
            self.ensure_body_element()?;
            self.insertion_mode = InsertionMode::InBody;
        }
        let parent = if self.insertion_mode == InsertionMode::InBody && self.should_foster_parent()
        {
            self.dom
                .parent(self.current_node())
                .map_err(TreeBuildError::Dom)?
                .unwrap_or(self.current_node())
        } else {
            self.current_node()
        };
        self.dom
            .append_text(parent, data)
            .map_err(TreeBuildError::Dom)?;
        Ok(())
    }

    /*
     * is_doc_html_or_head -- true iff the given node is the document
     * itself, the root <html> element, or the <head> element.
     *
     * WHY: handle_text uses this to decide whether character data should
     * trigger implicit-body insertion (only when the text would land in
     * one of these structural positions) or be appended directly to a
     * deeper content element such as <title> or <style>.
     */
    fn is_doc_html_or_head(&self, node: NodeId) -> bool {
        if node == self.document {
            return true;
        }
        if Some(node) == self.html_element {
            return true;
        }
        if Some(node) == self.head_element {
            return true;
        }
        let Ok(node_ref) = self.dom.node(node) else {
            return false;
        };
        if let NodeKind::Element { name, .. } = node_ref.kind() {
            matches!(name.as_str(), "html" | "head")
        } else {
            true
        }
    }

    fn handle_comment(&mut self, data: &str) -> Result<(), TreeBuildError> {
        let comment = self.dom.create_comment(data);
        let parent = if self.insertion_mode == InsertionMode::Initial {
            self.document
        } else {
            self.current_node()
        };
        self.dom
            .append_child(parent, comment)
            .map_err(TreeBuildError::Dom)?;
        Ok(())
    }

    fn handle_doctype(
        &mut self,
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
    ) -> Result<(), TreeBuildError> {
        let doctype = self.dom.create_doctype(name, public_id, system_id);
        self.dom
            .append_child(self.document, doctype)
            .map_err(TreeBuildError::Dom)?;
        Ok(())
    }

    fn current_node(&self) -> NodeId {
        *self.open_elements.last().expect("document node present")
    }

    fn insert_element(
        &mut self,
        parent: NodeId,
        name: &str,
        attributes: &[crate::Attribute],
        self_closing: bool,
    ) -> Result<NodeId, TreeBuildError> {
        let element = self.dom.create_element(name);
        self.dom
            .append_child(parent, element)
            .map_err(TreeBuildError::Dom)?;
        for attr in attributes {
            let value = attr.value.as_deref().unwrap_or("");
            self.dom
                .set_attribute(element, &attr.name, value)
                .map_err(TreeBuildError::Dom)?;
        }
        /*
         * HTML5 8.1.2: void elements have no end tag and accept no
         * children. They must never be pushed onto the open-elements
         * stack regardless of whether the source used the XHTML-style
         * "<br />" self-closing slash. Doing otherwise causes following
         * siblings (e.g. a <button> after an <input>) to be misparented
         * as descendants of the void element.
         *
         * Spec list: area, base, br, col, embed, hr, img, input, link,
         * meta, source, track, wbr (HTML Living Standard "void elements"
         * definition). Keep this in sync with is_void_element below.
         */
        if !self_closing && !is_void_element(name) {
            self.open_elements.push(element);
        }
        Ok(element)
    }

    fn insert_html_element(
        &mut self,
        attributes: &[crate::Attribute],
        self_closing: bool,
    ) -> Result<NodeId, TreeBuildError> {
        if let Some(html) = self.html_element {
            return Ok(html);
        }
        let element = self.dom.create_element("html");
        self.dom
            .append_child(self.document, element)
            .map_err(TreeBuildError::Dom)?;
        for attr in attributes {
            let value = attr.value.as_deref().unwrap_or("");
            self.dom
                .set_attribute(element, &attr.name, value)
                .map_err(TreeBuildError::Dom)?;
        }
        if !self_closing {
            self.open_elements.push(element);
        }
        self.html_element = Some(element);
        Ok(element)
    }

    fn insert_head_element(
        &mut self,
        attributes: &[crate::Attribute],
        self_closing: bool,
    ) -> Result<NodeId, TreeBuildError> {
        if let Some(head) = self.head_element {
            return Ok(head);
        }
        let html = self.ensure_html_element()?;
        let element = self.dom.create_element("head");
        self.dom
            .append_child(html, element)
            .map_err(TreeBuildError::Dom)?;
        for attr in attributes {
            let value = attr.value.as_deref().unwrap_or("");
            self.dom
                .set_attribute(element, &attr.name, value)
                .map_err(TreeBuildError::Dom)?;
        }
        if !self_closing {
            self.open_elements.push(element);
        }
        self.head_element = Some(element);
        Ok(element)
    }

    fn insert_body_element(
        &mut self,
        attributes: &[crate::Attribute],
        self_closing: bool,
    ) -> Result<NodeId, TreeBuildError> {
        if let Some(body) = self.body_element {
            return Ok(body);
        }
        let html = self.ensure_html_element()?;
        let element = self.dom.create_element("body");
        self.dom
            .append_child(html, element)
            .map_err(TreeBuildError::Dom)?;
        for attr in attributes {
            let value = attr.value.as_deref().unwrap_or("");
            self.dom
                .set_attribute(element, &attr.name, value)
                .map_err(TreeBuildError::Dom)?;
        }
        if !self_closing {
            self.open_elements.push(element);
        }
        self.body_element = Some(element);
        Ok(element)
    }

    fn ensure_html_element(&mut self) -> Result<NodeId, TreeBuildError> {
        if let Some(html) = self.html_element {
            return Ok(html);
        }
        self.insert_html_element(&[], false)
    }

    fn ensure_head_element(&mut self) -> Result<NodeId, TreeBuildError> {
        if let Some(head) = self.head_element {
            return Ok(head);
        }
        self.insert_head_element(&[], false)
    }

    fn ensure_body_element(&mut self) -> Result<NodeId, TreeBuildError> {
        if let Some(body) = self.body_element {
            return Ok(body);
        }
        self.insert_body_element(&[], false)
    }

    fn pop_until_tag(&mut self, name: &str) -> bool {
        for idx in (0..self.open_elements.len()).rev() {
            let node_id = self.open_elements[idx];
            if let Ok(node) = self.dom.node(node_id) {
                if let NodeKind::Element {
                    name: node_name, ..
                } = node.kind()
                {
                    if node_name.as_str().eq_ignore_ascii_case(name) {
                        self.open_elements.truncate(idx);
                        return true;
                    }
                }
            }
        }
        false
    }

    fn should_foster_parent(&self) -> bool {
        let current = self.current_node();
        let Ok(node) = self.dom.node(current) else {
            return false;
        };
        if let NodeKind::Element { name, .. } = node.kind() {
            matches!(name.as_str(), "table" | "tbody" | "thead" | "tfoot" | "tr")
        } else {
            false
        }
    }
}

impl From<DomError> for TreeBuildError {
    fn from(error: DomError) -> Self {
        TreeBuildError::Dom(error)
    }
}

fn is_head_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "base" | "link" | "meta" | "title" | "style" | "script"
    )
}

/*
 * is_void_element -- HTML5 Living Standard "void elements" predicate.
 *
 * WHY: void elements terminate without an end tag and never accept
 * children. The tree builder must NOT push them onto the open-elements
 * stack, otherwise sibling tags (e.g. a <button> following an <input>)
 * become misparented as descendants of the void element.
 *
 * Source: https://html.spec.whatwg.org/multipage/syntax.html#void-elements
 *
 * Keep this list in sync with insert_element's gating logic.
 */
fn is_void_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}
