//! JS fetch() API -- returns a Promise that resolves to a Response object.
//!
//! Runs the HTTP request on a background thread to avoid blocking the VM.
//! Completion enqueues a microtask to resolve the Promise.

use std::cell::RefCell;
use std::rc::Rc;

use silksurf_net::NetClient;

use super::native_fn;
use crate::vm::promise::{self, MicrotaskQueue, Promise};
use crate::vm::value::{NativeFunction, Object, PropertyKey, Value};

/// Install the global fetch() function.
pub fn install(global: &mut Object) {
    // Shared list of pending fetches for the event loop to poll
    // In a full impl, this would be on the Vm struct
    global.set_by_str("fetch", native_fn("fetch", fetch_fn));
}

fn fetch_fn(args: &[Value]) -> Value {
    let url = args
        .first()
        .map(|v| {
            let s = v.to_js_string();
            s.as_str().unwrap_or("").to_string()
        })
        .unwrap_or_default();

    // Extract options (method, headers, body)
    let method = args
        .get(1)
        .and_then(|opts| {
            if let Value::Object(o) = opts {
                let m = o.borrow().get_by_str("method");
                m.as_js_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "GET".to_string());

    let body = args
        .get(1)
        .and_then(|opts| {
            if let Value::Object(o) = opts {
                let b = o.borrow().get_by_str("body");
                let s = b.to_js_string();
                Some(s.as_str().unwrap_or("").as_bytes().to_vec())
            } else {
                None
            }
        })
        .unwrap_or_default();

    // Create a Promise for the result
    let promise = Promise::new();
    let mut queue = MicrotaskQueue::new();

    // Perform synchronous fetch (in a full impl, this would be async on a thread)
    // For now, we do it inline to avoid complexity with thread -> Rc bridging
    let client = silksurf_net::BasicClient::new();
    let http_method = match method.to_ascii_uppercase().as_str() {
        "POST" => silksurf_net::HttpMethod::Post,
        "PUT" => silksurf_net::HttpMethod::Put,
        "DELETE" => silksurf_net::HttpMethod::Delete,
        _ => silksurf_net::HttpMethod::Get,
    };

    let request = silksurf_net::HttpRequest {
        method: http_method,
        url: url.clone(),
        headers: vec![("Accept".to_string(), "*/*".to_string())],
        body,
    };

    match client.fetch(&request) {
        Ok(response) => {
            let response_obj = make_response_object(response);
            Promise::resolve(&promise, response_obj, &mut queue);
        }
        Err(err) => {
            Promise::reject(&promise, Value::string_owned(err.message), &mut queue);
        }
    }

    queue.drain();
    promise::promise_to_value(&promise)
}

/// Create a JS Response-like object from an HTTP response.
fn make_response_object(response: silksurf_net::HttpResponse) -> Value {
    let status = response.status;
    let headers = response.headers.clone();
    let body_bytes = response.body;

    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));

    {
        let mut o = obj_rc.borrow_mut();
        o.set_by_key(
            PropertyKey::from_str("status"),
            Value::Number(f64::from(status)),
        );
        o.set_by_key(
            PropertyKey::from_str("ok"),
            Value::Boolean((200..300).contains(&status)),
        );
        o.set_by_key(
            PropertyKey::from_str("statusText"),
            Value::string(match status {
                200 => "OK",
                301 => "Moved Permanently",
                302 => "Found",
                304 => "Not Modified",
                400 => "Bad Request",
                401 => "Unauthorized",
                403 => "Forbidden",
                404 => "Not Found",
                500 => "Internal Server Error",
                _ => "",
            }),
        );

        // .headers (simple object)
        let headers_obj = Object::new();
        let headers_rc = Rc::new(RefCell::new(headers_obj));
        {
            let mut h = headers_rc.borrow_mut();
            for (name, value) in &headers {
                h.set_by_str(&name.to_ascii_lowercase(), Value::string(value));
            }
        }
        o.set_by_key(PropertyKey::from_str("headers"), Value::Object(headers_rc));

        // .text() -> Promise<string>
        let body_for_text = body_bytes.clone();
        o.set_by_key(
            PropertyKey::from_str("text"),
            Value::NativeFunction(Rc::new(NativeFunction::new("text", move |_args| {
                let text = String::from_utf8_lossy(&body_for_text).to_string();
                // Return a resolved promise
                let p = Promise::new();
                let mut q = MicrotaskQueue::new();
                Promise::resolve(&p, Value::string_owned(text), &mut q);
                q.drain();
                promise::promise_to_value(&p)
            }))),
        );

        // .json() -> Promise<object>
        let body_for_json = body_bytes;
        o.set_by_key(
            PropertyKey::from_str("json"),
            Value::NativeFunction(Rc::new(NativeFunction::new("json", move |_args| {
                let text = String::from_utf8_lossy(&body_for_json);
                let p = Promise::new();
                let mut q = MicrotaskQueue::new();
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(parsed) => {
                        let js_val = crate::vm::builtins::json::serde_to_js_public(&parsed);
                        Promise::resolve(&p, js_val, &mut q);
                    }
                    Err(e) => {
                        Promise::reject(
                            &p,
                            Value::string_owned(format!("JSON parse error: {e}")),
                            &mut q,
                        );
                    }
                }
                q.drain();
                promise::promise_to_value(&p)
            }))),
        );
    }

    Value::Object(obj_rc)
}
