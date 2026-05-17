use silksurf_dom::{Dom, NodeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsValue {
    Undefined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsError {
    pub message: String,
}

impl From<JsError> for silksurf_core::SilkError {
    fn from(e: JsError) -> Self {
        silksurf_core::SilkError::Js(e.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsTask {
    Script(String),
}

pub trait JsRuntime {
    fn bind_dom(&mut self, dom: &Dom, document: NodeId) -> Result<(), JsError>;
    fn evaluate(&mut self, source: &str) -> Result<JsValue, JsError>;
    fn enqueue_task(&mut self, task: JsTask);
    fn run_microtasks(&mut self) -> Result<(), JsError>;
}

pub struct NoopJsRuntime {
    tasks: Vec<JsTask>,
}

impl NoopJsRuntime {
    #[must_use] 
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    #[must_use] 
    pub fn pending_tasks(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for NoopJsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl JsRuntime for NoopJsRuntime {
    fn bind_dom(&mut self, _dom: &Dom, _document: NodeId) -> Result<(), JsError> {
        Ok(())
    }

    fn evaluate(&mut self, _source: &str) -> Result<JsValue, JsError> {
        Ok(JsValue::Undefined)
    }

    fn enqueue_task(&mut self, task: JsTask) {
        self.tasks.push(task);
    }

    fn run_microtasks(&mut self) -> Result<(), JsError> {
        self.tasks.clear();
        Ok(())
    }
}
