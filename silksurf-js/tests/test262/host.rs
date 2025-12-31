//! $262 host object for test262 conformance tests
//!
//! Implements the host-defined $262 object required by test262.
//! See: https://github.com/tc39/test262/blob/main/INTERPRETING.md

use silksurf_js::vm::{Value, Vm};
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag for async test completion
static ASYNC_DONE: AtomicBool = AtomicBool::new(false);

/// $262 host object providing test262-required functionality
#[derive(Debug)]
pub struct Host262 {
    /// Captured print output
    pub print_output: Vec<String>,
    /// Whether createRealm was called
    pub realm_created: bool,
    /// Detached ArrayBuffer instances
    pub detached_buffers: Vec<usize>,
    /// Agent info
    pub agent: AgentInfo,
}

/// Agent information for $262.agent
#[derive(Debug, Default)]
pub struct AgentInfo {
    /// Agent start function called
    pub started: bool,
    /// Broadcast data
    pub broadcast_data: Option<Vec<u8>>,
    /// Report output
    pub reports: Vec<String>,
}

impl Default for Host262 {
    fn default() -> Self {
        Self::new()
    }
}

impl Host262 {
    pub fn new() -> Self {
        ASYNC_DONE.store(false, Ordering::SeqCst);
        Self {
            print_output: Vec::new(),
            realm_created: false,
            detached_buffers: Vec::new(),
            agent: AgentInfo::default(),
        }
    }

    /// Reset state between tests
    pub fn reset(&mut self) {
        self.print_output.clear();
        self.realm_created = false;
        self.detached_buffers.clear();
        self.agent = AgentInfo::default();
        ASYNC_DONE.store(false, Ordering::SeqCst);
    }

    /// $262.createRealm()
    /// Creates a new ECMAScript realm and returns its global object
    pub fn create_realm(&mut self) -> Value {
        self.realm_created = true;
        // Return a new global-like object
        // Full implementation would create isolated realm
        Value::Undefined
    }

    /// $262.detachArrayBuffer(buffer)
    /// Detaches an ArrayBuffer, making it unusable
    pub fn detach_array_buffer(&mut self, _buffer: &Value) {
        // Track detachment (full impl would actually detach)
        self.detached_buffers.push(self.detached_buffers.len());
    }

    /// $262.evalScript(code)
    /// Evaluates script in the current realm
    pub fn eval_script(&self, _vm: &mut Vm, _code: &str) -> Value {
        // Would parse and execute code
        Value::Undefined
    }

    /// $262.gc()
    /// Hints garbage collection (implementation-defined)
    pub fn gc(&self) {
        // Trigger GC if available
    }

    /// $262.global
    /// Returns the global object
    pub fn global(&self, vm: &Vm) -> Value {
        Value::Object(vm.global.clone())
    }

    /// $262.IsHTMLDDA
    /// Returns the [[IsHTMLDDA]] exotic object
    pub fn is_html_dda(&self) -> Value {
        // Special object that typeof returns "undefined"
        // Used for document.all compatibility
        Value::Null // Simplified - real impl needs exotic object
    }

    /// $262.agent.start(script)
    /// Starts a new agent (worker thread)
    pub fn agent_start(&mut self, _script: &str) {
        self.agent.started = true;
    }

    /// $262.agent.broadcast(buffer)
    /// Broadcasts SharedArrayBuffer to all agents
    pub fn agent_broadcast(&mut self, data: Vec<u8>) {
        self.agent.broadcast_data = Some(data);
    }

    /// $262.agent.getReport()
    /// Gets a report from an agent
    pub fn agent_get_report(&mut self) -> Option<String> {
        self.agent.reports.pop()
    }

    /// $262.agent.sleep(ms)
    /// Sleeps for specified milliseconds
    pub fn agent_sleep(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    /// $262.agent.monotonicNow()
    /// Returns monotonic timestamp
    pub fn agent_monotonic_now(&self) -> f64 {
        use std::time::Instant;
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// print() function for test output
    pub fn print(&mut self, message: &str) {
        self.print_output.push(message.to_string());
    }

    /// $DONE() for async tests
    pub fn done(error: Option<&str>) {
        if let Some(e) = error {
            eprintln!("$DONE error: {}", e);
        }
        ASYNC_DONE.store(true, Ordering::SeqCst);
    }

    /// Check if async test completed
    pub fn is_done() -> bool {
        ASYNC_DONE.load(Ordering::SeqCst)
    }
}

/// Standard harness includes content
pub mod harness_files {
    /// assert.js - Basic assertion functions
    pub const ASSERT_JS: &str = r#"
function assert(condition, message) {
    if (!condition) {
        throw new Test262Error(message || "Assertion failed");
    }
}

assert.sameValue = function(actual, expected, message) {
    if (!Object.is(actual, expected)) {
        throw new Test262Error(
            (message ? message + ": " : "") +
            "Expected " + String(expected) + " but got " + String(actual)
        );
    }
};

assert.notSameValue = function(actual, unexpected, message) {
    if (Object.is(actual, unexpected)) {
        throw new Test262Error(
            (message ? message + ": " : "") +
            "Unexpected value: " + String(actual)
        );
    }
};

assert.throws = function(errorConstructor, fn, message) {
    var threw = false;
    var error;
    try {
        fn();
    } catch (e) {
        threw = true;
        error = e;
    }
    if (!threw) {
        throw new Test262Error((message ? message + ": " : "") + "Expected exception");
    }
    if (!(error instanceof errorConstructor)) {
        throw new Test262Error(
            (message ? message + ": " : "") +
            "Expected " + errorConstructor.name + " but got " + error.constructor.name
        );
    }
};
"#;

    /// sta.js - Standard test assertions
    pub const STA_JS: &str = r#"
var Test262Error = function(message) {
    this.message = message || "";
};

Test262Error.prototype.toString = function() {
    return "Test262Error: " + this.message;
};

function $ERROR(message) {
    throw new Test262Error(message);
}

function $DONEPRINTHANDLE(value) {
    print(value);
}
"#;

    /// doneprintHandle.js - Async test completion
    pub const DONE_PRINT_HANDLE_JS: &str = r#"
function $DONE(error) {
    if (error) {
        if (typeof error === 'object' && error !== null && 'stack' in error) {
            print("Test262:AsyncTestFailure:" + error.stack);
        } else {
            print("Test262:AsyncTestFailure:" + String(error));
        }
    } else {
        print("Test262:AsyncTestComplete");
    }
}
"#;

    /// compareArray.js - Array comparison
    pub const COMPARE_ARRAY_JS: &str = r#"
function compareArray(actual, expected) {
    if (actual.length !== expected.length) {
        return false;
    }
    for (var i = 0; i < actual.length; i++) {
        if (actual[i] !== expected[i]) {
            return false;
        }
    }
    return true;
}

assert.compareArray = function(actual, expected, message) {
    if (!compareArray(actual, expected)) {
        throw new Test262Error(
            (message ? message + ": " : "") +
            "Arrays not equal: " + String(actual) + " vs " + String(expected)
        );
    }
};
"#;

    /// propertyHelper.js - Property descriptor helpers
    pub const PROPERTY_HELPER_JS: &str = r#"
function verifyProperty(obj, name, desc) {
    var actual = Object.getOwnPropertyDescriptor(obj, name);
    if (actual === undefined) {
        throw new Test262Error("Property " + String(name) + " not found");
    }
    if (desc.writable !== undefined && actual.writable !== desc.writable) {
        throw new Test262Error("writable mismatch");
    }
    if (desc.enumerable !== undefined && actual.enumerable !== desc.enumerable) {
        throw new Test262Error("enumerable mismatch");
    }
    if (desc.configurable !== undefined && actual.configurable !== desc.configurable) {
        throw new Test262Error("configurable mismatch");
    }
}

function verifyNotWritable(obj, name) {
    var desc = Object.getOwnPropertyDescriptor(obj, name);
    if (desc && desc.writable) {
        throw new Test262Error("Expected non-writable");
    }
}

function verifyNotEnumerable(obj, name) {
    var desc = Object.getOwnPropertyDescriptor(obj, name);
    if (desc && desc.enumerable) {
        throw new Test262Error("Expected non-enumerable");
    }
}

function verifyNotConfigurable(obj, name) {
    var desc = Object.getOwnPropertyDescriptor(obj, name);
    if (desc && desc.configurable) {
        throw new Test262Error("Expected non-configurable");
    }
}
"#;

    /// Get harness file content by name
    pub fn get(name: &str) -> Option<&'static str> {
        match name {
            "assert.js" => Some(ASSERT_JS),
            "sta.js" => Some(STA_JS),
            "doneprintHandle.js" => Some(DONE_PRINT_HANDLE_JS),
            "compareArray.js" => Some(COMPARE_ARRAY_JS),
            "propertyHelper.js" => Some(PROPERTY_HELPER_JS),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host262_creation() {
        let host = Host262::new();
        assert!(host.print_output.is_empty());
        assert!(!host.realm_created);
    }

    #[test]
    fn test_host262_print() {
        let mut host = Host262::new();
        host.print("hello");
        host.print("world");
        assert_eq!(host.print_output, vec!["hello", "world"]);
    }

    #[test]
    fn test_host262_reset() {
        let mut host = Host262::new();
        host.print("test");
        host.realm_created = true;
        host.reset();
        assert!(host.print_output.is_empty());
        assert!(!host.realm_created);
    }

    #[test]
    fn test_async_done() {
        Host262::new(); // Reset static
        assert!(!Host262::is_done());
        Host262::done(None);
        assert!(Host262::is_done());
    }

    #[test]
    fn test_harness_files() {
        assert!(harness_files::get("assert.js").is_some());
        assert!(harness_files::get("sta.js").is_some());
        assert!(harness_files::get("unknown.js").is_none());
    }
}
