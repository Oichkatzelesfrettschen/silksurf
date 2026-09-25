use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::{JsObject, ObjectInitializer},
    property::Attribute,
};

use silksurf_dom::Dom;

use super::{SilkContext, event_dispatch};

#[derive(Clone, Default)]
pub struct WindowMessageHub(Arc<Mutex<MessageHubState>>);

#[derive(Clone)]
pub struct WindowMessageContext {
    hub: WindowMessageHub,
    id: u64,
    origin: WindowOrigin,
    parent: Option<(u64, u64)>,
    _registration: Arc<ContextRegistration>,
}

struct ContextRegistration {
    hub: WindowMessageHub,
    id: u64,
}

impl Drop for ContextRegistration {
    fn drop(&mut self) {
        let mut state = self.hub.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.origins.remove(&self.id);
        state.parents.remove(&self.id);
        state.pending.remove(&self.id);
        state
            .frame_contexts
            .retain(|(parent, _), child| *parent != self.id && *child != self.id);
        for queue in state.pending.values_mut() {
            queue.retain(|message| message.source_context != self.id);
        }
    }
}

struct MessageHubState {
    next_context_id: u64,
    origins: HashMap<u64, WindowOrigin>,
    parents: HashMap<u64, u64>,
    frame_contexts: HashMap<(u64, u64), u64>,
    pending: HashMap<u64, VecDeque<WindowMessage>>,
}

#[derive(Clone)]
struct WindowMessage {
    source_context: u64,
    source_frame: Option<u64>,
    origin: String,
    target_origin: MessageTargetOrigin,
    payload: MessagePayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OriginKey {
    Tuple(String),
    Opaque(u64),
}

#[derive(Clone, Debug)]
struct WindowOrigin {
    key: OriginKey,
    serialized: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MessageTargetOrigin {
    Any,
    Exact(OriginKey),
}

#[derive(Clone)]
enum MessagePayload {
    Undefined,
    Json(serde_json::Value),
}

impl Default for MessageHubState {
    fn default() -> Self {
        Self {
            next_context_id: 1,
            origins: HashMap::new(),
            parents: HashMap::new(),
            frame_contexts: HashMap::new(),
            pending: HashMap::new(),
        }
    }
}

impl MessageTargetOrigin {
    fn matches(&self, target: &OriginKey) -> bool {
        matches!(self, Self::Any) || matches!(self, Self::Exact(origin) if origin == target)
    }

    fn label(&self) -> String {
        match self {
            Self::Any => "*".to_string(),
            Self::Exact(OriginKey::Tuple(origin)) => origin.clone(),
            Self::Exact(OriginKey::Opaque(_)) => "null".to_string(),
        }
    }
}

impl WindowMessageHub {
    pub fn create_context(
        &self,
        url: &str,
        parent: Option<(u64, u64)>,
        opaque_origin: bool,
    ) -> WindowMessageContext {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let id = state.next_context_id;
        state.next_context_id = state.next_context_id.saturating_add(1);
        let origin = if opaque_origin {
            WindowOrigin {
                key: OriginKey::Opaque(id),
                serialized: "null".to_string(),
            }
        } else if let Some(serialized) = url::Url::parse(url)
            .ok()
            .map(|url| url.origin().ascii_serialization())
            .filter(|origin| origin != "null")
        {
            WindowOrigin {
                key: OriginKey::Tuple(serialized.clone()),
                serialized,
            }
        } else {
            WindowOrigin {
                key: OriginKey::Opaque(id),
                serialized: "null".to_string(),
            }
        };
        state.origins.insert(id, origin.clone());
        state.pending.insert(id, VecDeque::new());
        if let Some((parent_context, owner_node)) = parent {
            state.parents.insert(id, parent_context);
            state
                .frame_contexts
                .insert((parent_context, owner_node), id);
        }
        let registration = Arc::new(ContextRegistration {
            hub: self.clone(),
            id,
        });
        WindowMessageContext {
            hub: self.clone(),
            id,
            origin,
            parent,
            _registration: registration,
        }
    }
}

impl WindowMessageContext {
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    fn target_for_frame(&self, owner_node: u64) -> Option<u64> {
        self.hub
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .frame_contexts
            .get(&(self.id, owner_node))
            .copied()
    }

    fn top_context_id(&self) -> u64 {
        let state = self.hub.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut context_id = self.id;
        while let Some(parent_context) = state.parents.get(&context_id) {
            context_id = *parent_context;
        }
        context_id
    }

    fn post(
        &self,
        target_context: u64,
        source_frame: Option<u64>,
        payload: MessagePayload,
        target_origin: MessageTargetOrigin,
    ) {
        let mut state = self.hub.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(origin) = state.origins.get(&target_context).cloned() else {
            trace_window_message(
                "drop-target",
                self.id,
                target_context,
                &payload,
                &self.origin.serialized,
            );
            return;
        };
        if !target_origin.matches(&origin.key) {
            trace_window_message(
                "drop-origin",
                self.id,
                target_context,
                &payload,
                &target_origin.label(),
            );
            return;
        }
        if let Some(queue) = state.pending.get_mut(&target_context) {
            trace_window_message(
                "queue",
                self.id,
                target_context,
                &payload,
                &target_origin.label(),
            );
            queue.push_back(WindowMessage {
                source_context: self.id,
                source_frame,
                origin: self.origin.serialized.clone(),
                target_origin,
                payload,
            });
        }
    }

    fn post_to_frame(
        &self,
        owner_node: u64,
        payload: MessagePayload,
        target_origin: MessageTargetOrigin,
    ) {
        if let Some(target_context) = self.target_for_frame(owner_node) {
            self.post(target_context, None, payload, target_origin);
        }
    }

    fn take_messages(&self) -> Vec<WindowMessage> {
        self.hub
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .get_mut(&self.id)
            .map_or_else(Vec::new, |queue| queue.drain(..).collect())
    }

    fn proxy_for_context(&self, context_id: u64, ctx: &mut Context) -> JsResult<JsObject> {
        cached_proxy(&format!("context:{context_id}"), ctx).map_or_else(
            || {
                let endpoint = self.clone();
                let source_origin = self.origin.key.clone();
                let parent_owner = self
                    .parent
                    .filter(|(parent_context, _)| *parent_context == context_id)
                    .map(|(_, owner)| owner);
                let object = make_window_proxy(
                    source_origin,
                    move |payload, target_origin| {
                        endpoint.post(context_id, parent_owner, payload, target_origin);
                    },
                    ctx,
                );
                cache_proxy(&format!("context:{context_id}"), &object, ctx)?;
                Ok(object)
            },
            Ok,
        )
    }

    fn proxy_for_frame(&self, owner_node: u64, ctx: &mut Context) -> JsResult<JsObject> {
        cached_proxy(&format!("frame:{owner_node}"), ctx).map_or_else(
            || {
                let endpoint = self.clone();
                let object = make_window_proxy(
                    self.origin.key.clone(),
                    move |payload, target_origin| {
                        endpoint.post_to_frame(owner_node, payload, target_origin);
                    },
                    ctx,
                );
                cache_proxy(&format!("frame:{owner_node}"), &object, ctx)?;
                Ok(object)
            },
            Ok,
        )
    }

    fn install(&self, context: &mut Context) -> JsResult<()> {
        let endpoint = self.clone();
        // SAFETY: the callback owns an Arc-backed endpoint and captures no JS value.
        let frame_proxy_factory = unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let owner_node = args
                    .first()
                    .map(|value| value.to_string(ctx))
                    .transpose()?
                    .map(|value| value.to_std_string_lossy().parse::<u64>())
                    .transpose()
                    .map_err(|_| boa_engine::JsNativeError::typ().with_message("invalid frame id"))?
                    .unwrap_or_default();
                endpoint.proxy_for_frame(owner_node, ctx).map(Into::into)
            })
        };
        context.register_global_callable(
            js_string!("__silksurfWindowProxyForFrame"),
            1,
            frame_proxy_factory,
        )?;

        let endpoint = self.clone();
        // SAFETY: the callback owns an Arc-backed endpoint and captures no JS value.
        let own_post_message = unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let payload = message_data(args.first(), ctx)?;
                let target_origin =
                    parse_post_message_target_origin(args.get(1), &endpoint.origin.key, ctx)?;
                endpoint.post(endpoint.id, None, payload, target_origin);
                Ok(JsValue::undefined())
            })
        };
        context.register_global_callable(js_string!("postMessage"), 2, own_post_message)?;

        let global = context.global_object().clone();
        let parent_window = match self.parent {
            Some((parent_context, _)) => self.proxy_for_context(parent_context, context)?,
            None => global.clone(),
        };
        let top_context_id = self.top_context_id();
        let top_window = if top_context_id == self.id {
            global
        } else {
            self.proxy_for_context(top_context_id, context)?
        };
        context.register_global_property(
            js_string!("parent"),
            parent_window.clone(),
            Attribute::all(),
        )?;
        context.register_global_property(js_string!("top"), top_window, Attribute::all())?;
        Ok(())
    }

    fn deliver(&self, dom: &Arc<Mutex<Dom>>, context: &mut Context) -> JsResult<usize> {
        let messages = self.take_messages();
        let mut delivered = 0;
        for message in messages {
            if !message.target_origin.matches(&self.origin.key) {
                continue;
            }
            let source = if self
                .parent
                .is_some_and(|(parent, _)| parent == message.source_context)
            {
                self.proxy_for_context(message.source_context, context)?
            } else if let Some(owner_node) = message.source_frame {
                self.proxy_for_frame(owner_node, context)?
            } else {
                self.proxy_for_context(message.source_context, context)?
            };
            trace_window_message(
                "deliver",
                message.source_context,
                self.id,
                &message.payload,
                &message.origin,
            );
            let payload = match &message.payload {
                MessagePayload::Undefined => JsValue::undefined(),
                MessagePayload::Json(value) => JsValue::from_json(value, context)?,
            };
            let event = ObjectInitializer::new(context)
                .property(js_string!("type"), js_string!("message"), Attribute::all())
                .property(js_string!("data"), payload, Attribute::all())
                .property(
                    js_string!("origin"),
                    JsString::from(message.origin.as_str()),
                    Attribute::all(),
                )
                .property(js_string!("source"), source, Attribute::all())
                .property(js_string!("isTrusted"), true, Attribute::all())
                .build();
            event_dispatch::dispatch_window_message(dom, &event, context)?;
            delivered += 1;
        }
        Ok(delivered)
    }
}

fn trace_window_message(
    action: &str,
    source_context: u64,
    target_context: u64,
    payload: &MessagePayload,
    origin: &str,
) {
    if std::env::var_os("SILKSURF_TRACE_WINDOW_MESSAGES").is_some() {
        let value = match payload {
            MessagePayload::Undefined => None,
            MessagePayload::Json(value) => Some(value),
        };
        let event = value
            .and_then(|value| value.get("event"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("other");
        let shape = value.map_or_else(
            || "undefined".to_string(),
            |value| {
                value.as_object().map_or_else(
                    || json_value_kind(value).to_string(),
                    |fields| {
                        let mut names = fields
                            .iter()
                            .map(|(name, value)| format!("{name}:{}", json_value_kind(value)))
                            .collect::<Vec<_>>();
                        names.sort();
                        format!("{{{}}}", names.join(","))
                    },
                )
            },
        );
        let reason = value
            .and_then(|value| value.get("reason"))
            .and_then(serde_json::Value::as_str)
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default();
        eprintln!(
            "[SilkSurf] Window message {action}: {event} {shape}{reason} source={source_context} target={target_context} origin={origin}"
        );
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

impl SilkContext {
    pub fn install_window_messages(&mut self, messages: WindowMessageContext) {
        match messages.install(&mut self.ctx) {
            Ok(()) => self.window_messages = Some(messages),
            Err(error) => eprintln!("silksurf-js: window message setup failed: {error}"),
        }
    }

    pub fn window_context_id(&self) -> Option<u64> {
        self.window_messages.as_ref().map(WindowMessageContext::id)
    }

    pub(super) fn deliver_window_messages(&mut self) -> Result<usize, String> {
        let (Some(messages), Some(dom)) = (&self.window_messages, &self.dom) else {
            return Ok(0);
        };
        messages
            .deliver(dom, &mut self.ctx)
            .map_err(|error| error.to_string())
    }
}

fn message_data(value: Option<&JsValue>, context: &mut Context) -> JsResult<MessagePayload> {
    let Some(value) = value else {
        return Ok(MessagePayload::Undefined);
    };
    if value.is_undefined() {
        return Ok(MessagePayload::Undefined);
    }
    value
        .to_json(context)?
        .map(MessagePayload::Json)
        .ok_or_else(|| {
            boa_engine::JsNativeError::typ()
                .with_message("message data cannot be cloned")
                .into()
        })
}

fn parse_post_message_target_origin(
    value: Option<&JsValue>,
    source_origin: &OriginKey,
    context: &mut Context,
) -> JsResult<MessageTargetOrigin> {
    let target = if let Some(value) = value {
        if value.is_undefined() {
            return Ok(MessageTargetOrigin::Exact(source_origin.clone()));
        }
        if let Some(options) = value.as_object() {
            let target_origin = options.get(js_string!("targetOrigin"), context)?;
            if target_origin.is_undefined() {
                return Ok(MessageTargetOrigin::Exact(source_origin.clone()));
            }
            target_origin.to_string(context)?.to_std_string_lossy()
        } else {
            value.to_string(context)?.to_std_string_lossy()
        }
    } else {
        return Ok(MessageTargetOrigin::Exact(source_origin.clone()));
    };
    if target == "*" {
        return Ok(MessageTargetOrigin::Any);
    }
    if target == "/" {
        return Ok(MessageTargetOrigin::Exact(source_origin.clone()));
    }
    let parsed = url::Url::parse(&target).map_err(|error| {
        boa_engine::JsNativeError::syntax().with_message(format!("invalid targetOrigin: {error}"))
    })?;
    let serialized = parsed.origin().ascii_serialization();
    if serialized == "null" {
        return Ok(MessageTargetOrigin::Exact(OriginKey::Opaque(u64::MAX)));
    }
    Ok(MessageTargetOrigin::Exact(OriginKey::Tuple(serialized)))
}

fn make_window_proxy(
    source_origin: OriginKey,
    send: impl Fn(MessagePayload, MessageTargetOrigin) + Send + 'static,
    context: &mut Context,
) -> JsObject {
    let send = Arc::new(send);
    // SAFETY: the callback owns its send closure and captures no JS value.
    let post_message = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let payload = message_data(args.first(), ctx)?;
            let target_origin = parse_post_message_target_origin(args.get(1), &source_origin, ctx)?;
            send(payload, target_origin);
            Ok(JsValue::undefined())
        })
    };
    ObjectInitializer::new(context)
        .function(post_message, js_string!("postMessage"), 2)
        .property(js_string!("closed"), false, Attribute::all())
        .build()
}

fn proxy_registry(context: &mut Context) -> JsResult<JsObject> {
    let global = context.global_object().clone();
    let key = js_string!("__silksurfWindowProxyCache");
    let existing = global.get(key.clone(), context)?;
    if let Some(object) = existing.as_object() {
        return Ok(object.clone());
    }
    let registry = ObjectInitializer::new(context).build();
    global.set(key, registry.clone(), false, context)?;
    Ok(registry)
}

fn cached_proxy(key: &str, context: &mut Context) -> Option<JsObject> {
    proxy_registry(context)
        .ok()?
        .get(JsString::from(key), context)
        .ok()?
        .as_object()
}

fn cache_proxy(key: &str, proxy: &JsObject, context: &mut Context) -> JsResult<()> {
    proxy_registry(context)?
        .set(JsString::from(key), proxy.clone(), false, context)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_post_message_delivers_origin_trust_and_stable_window_sources() {
        let mut dom = Dom::new();
        let document = dom.create_document();
        let html = dom.create_element("html");
        let body = dom.create_element("body");
        let frame = dom.create_element("iframe");
        dom.set_attribute(frame, "id", "frame")
            .expect("frame id sets");
        dom.append_child(document, html).expect("html attaches");
        dom.append_child(html, body).expect("body attaches");
        dom.append_child(body, frame).expect("frame attaches");
        dom.materialize_resolve_table();
        let parent_dom = Arc::new(Mutex::new(dom));
        let child_dom = Arc::new(Mutex::new(Dom::new()));
        let hub = WindowMessageHub::default();
        let parent_messages = hub.create_context("https://parent.test/", None, false);
        let child_messages = hub.create_context(
            "https://child.test/",
            Some((parent_messages.id(), frame.raw() as u64)),
            false,
        );
        let mut parent = SilkContext::with_dom(&parent_dom);
        let mut child = SilkContext::with_dom(&child_dom);
        parent.install_window_messages(parent_messages);
        child.install_window_messages(child_messages);
        assert!(parent.window_context_id().is_some());
        assert!(child.window_context_id().is_some());

        parent
            .eval(
                "globalThis.parentMessage = ''; window.addEventListener('message', function (event) { parentMessage = event.data.event + ':' + event.origin + ':' + (event.source === document.getElementById('frame').contentWindow) + ':' + event.isTrusted; }); document.getElementById('frame').contentWindow.postMessage({event: 'init'}, 'https://child.test');",
            )
            .expect("parent message posts");
        child
            .eval(
                "globalThis.childMessage = ''; window.addEventListener('message', function (event) { childMessage = event.data.event + ':' + event.origin + ':' + (event.source === window.parent) + ':' + event.isTrusted; }); parent.postMessage({event: 'ack'}, 'https://parent.test');",
            )
            .expect("child message posts");

        child
            .run_ready_host_callbacks()
            .expect("child receives parent message");
        child
            .eval("if (childMessage !== 'init:https://parent.test:true:true') throw new Error(childMessage);")
            .expect("child event carries parent origin and source");
        parent
            .run_ready_host_callbacks()
            .expect("parent receives child message");
        parent
            .eval("if (parentMessage !== 'ack:https://child.test:true:true') throw new Error(parentMessage);")
            .expect("parent event carries child origin and contentWindow identity");
    }

    #[test]
    fn post_message_defaults_to_sender_origin_and_accepts_undefined_data() {
        let parent_dom = Arc::new(Mutex::new(Dom::new()));
        let child_dom = Arc::new(Mutex::new(Dom::new()));
        let hub = WindowMessageHub::default();
        let parent_messages = hub.create_context("https://parent.test/", None, false);
        let child_messages = hub.create_context(
            "https://child.test/",
            Some((parent_messages.id(), 17)),
            false,
        );
        let mut parent = SilkContext::with_dom(&parent_dom);
        let mut child = SilkContext::with_dom(&child_dom);
        parent.install_window_messages(parent_messages);
        child.install_window_messages(child_messages);

        parent
            .eval(
                "globalThis.contentWindow = __silksurfWindowProxyForFrame('17'); contentWindow.postMessage('blocked'); contentWindow.postMessage('blocked-undefined', undefined); contentWindow.postMessage('allowed', '*');",
            )
            .expect("cross-origin posts evaluate");
        child
            .eval(
                "globalThis.received = []; window.addEventListener('message', event => received.push([event.data, event.origin])); parent.postMessage(undefined, 'https://parent.test');",
            )
            .expect("undefined data posts");
        child
            .run_ready_host_callbacks()
            .expect("child dispatches allowed cross-origin message");
        child
            .eval(
                "if (received.length !== 1 || received[0][0] !== 'allowed' || received[0][1] !== 'https://parent.test') throw new Error(JSON.stringify(received));",
            )
            .expect("omitted and undefined target origins use sender origin");

        parent
            .eval(
                "globalThis.undefinedReceived = false; window.addEventListener('message', event => { undefinedReceived = event.data === undefined && event.origin === 'https://child.test'; });",
            )
            .expect("parent installs undefined receiver");
        parent
            .run_ready_host_callbacks()
            .expect("child undefined post reaches matching parent origin");
        parent
            .eval("if (!undefinedReceived) throw new Error('undefined data was not preserved');")
            .expect("undefined data survives delivery");
    }

    #[test]
    fn nested_top_targets_the_root_context_while_parent_targets_the_direct_context() {
        let hub = WindowMessageHub::default();
        let root_dom = Arc::new(Mutex::new(Dom::new()));
        let middle_dom = Arc::new(Mutex::new(Dom::new()));
        let child_dom = Arc::new(Mutex::new(Dom::new()));
        let root_messages = hub.create_context("https://root.test/", None, false);
        let middle_messages =
            hub.create_context("https://middle.test/", Some((root_messages.id(), 1)), false);
        let child_messages = hub.create_context(
            "https://child.test/",
            Some((middle_messages.id(), 2)),
            false,
        );
        let mut root = SilkContext::with_dom(&root_dom);
        let mut middle = SilkContext::with_dom(&middle_dom);
        let mut child = SilkContext::with_dom(&child_dom);
        root.install_window_messages(root_messages);
        middle.install_window_messages(middle_messages);
        child.install_window_messages(child_messages);

        root.eval("globalThis.rootMessages = []; window.addEventListener('message', event => rootMessages.push(event.data));")
            .expect("root listener installs");
        middle.eval("globalThis.middleMessages = []; window.addEventListener('message', event => middleMessages.push(event.data));")
            .expect("middle listener installs");
        middle.eval("if (window.top !== window.parent) throw new Error('direct child top differs from direct parent'); window.top.postMessage('middle-to-root', 'https://root.test');")
            .expect("middle posts to top");
        child.eval("if (window.parent === window.top || window.top !== window.top) throw new Error('nested window aliases are unstable'); window.parent.postMessage('child-to-middle', 'https://middle.test'); window.top.postMessage('child-to-root', 'https://root.test');")
            .expect("grandchild posts to parent and top");
        middle
            .run_ready_host_callbacks()
            .expect("middle receives direct-child message");
        root.run_ready_host_callbacks()
            .expect("root receives top-level messages");
        middle.eval("if (middleMessages.length !== 1 || middleMessages[0] !== 'child-to-middle') throw new Error(JSON.stringify(middleMessages));")
            .expect("parent proxy addresses immediate parent");
        root.eval("if (rootMessages.length !== 2 || rootMessages[0] !== 'middle-to-root' || rootMessages[1] !== 'child-to-root') throw new Error(JSON.stringify(rootMessages));")
            .expect("top proxy addresses root browsing context");
    }

    #[test]
    fn dropped_frame_context_releases_registry_entries_after_the_last_clone() {
        let hub = WindowMessageHub::default();
        let parent = hub.create_context("https://parent.test/", None, false);
        let child = hub.create_context("https://child.test/", Some((parent.id(), 17)), false);
        let child_id = child.id();
        let child_clone = child.clone();
        parent.post_to_frame(
            17,
            MessagePayload::Json(serde_json::json!({"event": "to-child"})),
            MessageTargetOrigin::Any,
        );
        child.post(
            parent.id(),
            Some(17),
            MessagePayload::Json(serde_json::json!({"event": "to-parent"})),
            MessageTargetOrigin::Any,
        );

        drop(child);
        assert!(
            hub.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .origins
                .contains_key(&child_id)
        );
        drop(child_clone);

        let state = hub.0.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(state.origins.len(), 1);
        assert!(!state.pending.contains_key(&child_id));
        assert!(state.frame_contexts.is_empty());
        assert!(state.pending[&parent.id()].is_empty());
    }
}
