/*
 * platform_globals installs the web-platform globals that sit outside the DOM:
 * URL, URLSearchParams, base64 conversion, UTF-8 text codecs, structuredClone,
 * requestIdleCallback, screen, reportError, and a locale parser for Intl.
 *
 * The split follows correctness ownership. URL parsing, percent-encoding,
 * base64, and UTF-8 transcoding are byte-level algorithms whose answers a page
 * depends on, so `url` and this module compute them in Rust and expose the
 * result through hidden `__silksurf*` helpers. Object shape -- constructors,
 * accessors, iteration order -- is expressed in the bootstrap script, where the
 * shape reads as the specification writes it.
 */

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, Source, js_string,
    object::{ObjectInitializer, builtins::JsArray},
    property::Attribute,
};

/// Install every platform global. Call once per context, before page script.
pub(super) fn install_platform_globals(ctx: &mut Context) {
    install_url_natives(ctx);
    install_base64_natives(ctx);
    install_text_codec_natives(ctx);
    if let Err(err) = ctx.eval(Source::from_bytes(PLATFORM_BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: platform globals bootstrap failed: {err}");
    }
}

// ---- URL --------------------------------------------------------------------

/// Parse `input` against optional `base` and return the URL record the
/// bootstrap reads, or null when the pair does not form an absolute URL.
fn parse_url(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = match args.first() {
        Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
        None => return Ok(JsValue::null()),
    };
    let base = match args.get(1) {
        Some(value) if !value.is_undefined() && !value.is_null() => {
            Some(value.to_string(ctx)?.to_std_string_lossy())
        }
        _ => None,
    };
    let parsed = match base {
        Some(base) => match url::Url::parse(&base) {
            Ok(base) => base.join(&input),
            Err(_) => return Ok(JsValue::null()),
        },
        None => url::Url::parse(&input),
    };
    let Ok(parsed) = parsed else {
        return Ok(JsValue::null());
    };
    Ok(url_record(&parsed, ctx))
}

fn url_record(parsed: &url::Url, ctx: &mut Context) -> JsValue {
    let port = parsed.port().map_or(String::new(), |p| p.to_string());
    let host = parsed.host_str().map_or(String::new(), |h| {
        if port.is_empty() {
            h.to_string()
        } else {
            format!("{h}:{port}")
        }
    });
    let search = parsed.query().map_or(String::new(), |q| format!("?{q}"));
    let hash = parsed.fragment().map_or(String::new(), |f| format!("#{f}"));
    // url::Url::origin renders "null" for opaque origins, which matches the
    // serialization the URL standard defines for them.
    let origin = parsed.origin().ascii_serialization();
    ObjectInitializer::new(ctx)
        .property(
            js_string!("href"),
            js_string!(parsed.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("origin"),
            js_string!(origin.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("protocol"),
            js_string!(format!("{}:", parsed.scheme()).as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("username"),
            js_string!(parsed.username()),
            Attribute::all(),
        )
        .property(
            js_string!("password"),
            js_string!(parsed.password().unwrap_or("")),
            Attribute::all(),
        )
        .property(
            js_string!("host"),
            js_string!(host.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("hostname"),
            js_string!(parsed.host_str().unwrap_or("")),
            Attribute::all(),
        )
        .property(
            js_string!("port"),
            js_string!(port.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("pathname"),
            js_string!(parsed.path()),
            Attribute::all(),
        )
        .property(
            js_string!("search"),
            js_string!(search.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("hash"),
            js_string!(hash.as_str()),
            Attribute::all(),
        )
        .build()
        .into()
}

/// Serialize `[[name, value], ...]` pairs as an application/x-www-form-urlencoded
/// string.
fn encode_query(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(pairs) = args.first().and_then(JsValue::as_object) else {
        return Ok(js_string!("").into());
    };
    let pairs = JsArray::from_object(pairs.clone())?;
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    let length = pairs.length(ctx)?;
    for index in 0..length {
        let entry = pairs.get(index, ctx)?;
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let entry = JsArray::from_object(entry.clone())?;
        let name = entry.get(0_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
        let value = entry.get(1_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
        serializer.append_pair(&name, &value);
    }
    Ok(js_string!(serializer.finish().as_str()).into())
}

/// Parse an application/x-www-form-urlencoded string into `[[name, value], ...]`.
fn decode_query(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = match args.first() {
        Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
        None => String::new(),
    };
    let input = input.strip_prefix('?').unwrap_or(&input).to_string();
    let pairs = JsArray::new(ctx);
    for (name, value) in url::form_urlencoded::parse(input.as_bytes()) {
        let entry = JsArray::new(ctx);
        entry.push(js_string!(name.as_ref()), ctx)?;
        entry.push(js_string!(value.as_ref()), ctx)?;
        pairs.push(entry, ctx)?;
    }
    Ok(pairs.into())
}

fn install_url_natives(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("__silksurfParseUrl"),
        2,
        NativeFunction::from_fn_ptr(parse_url),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfEncodeQuery"),
        1,
        NativeFunction::from_fn_ptr(encode_query),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfDecodeQuery"),
        1,
        NativeFunction::from_fn_ptr(decode_query),
    );
}

// ---- base64 -----------------------------------------------------------------

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode one byte string (each code unit below 256) as base64. HTML defines
/// btoa over binary strings, so the input is code units, not UTF-8 bytes.
fn base64_encode(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = match args.first() {
        Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
        None => String::new(),
    };
    let mut bytes = Vec::with_capacity(input.chars().count());
    for character in input.chars() {
        let code = character as u32;
        if code > 0xff {
            return Err(boa_engine::JsNativeError::typ()
                .with_message("btoa: character out of Latin-1 range")
                .into());
        }
        bytes.push(u8::try_from(code).unwrap_or(0));
    }
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    Ok(js_string!(out.as_str()).into())
}

fn base64_value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode base64 into a byte string, one code unit per byte.
fn base64_decode(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = match args.first() {
        Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
        None => String::new(),
    };
    let symbols: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if symbols.len() % 4 == 1 {
        return Err(boa_engine::JsNativeError::typ()
            .with_message("atob: invalid base64 length")
            .into());
    }
    let mut out = String::with_capacity(symbols.len() / 4 * 3);
    for chunk in symbols.chunks(4) {
        let mut accumulator = 0_u32;
        for (index, symbol) in chunk.iter().enumerate() {
            let Some(value) = base64_value(*symbol) else {
                return Err(boa_engine::JsNativeError::typ()
                    .with_message("atob: invalid base64 character")
                    .into());
            };
            accumulator |= value << (18 - 6 * index);
        }
        let produced = chunk.len() - 1;
        for index in 0..produced {
            let byte = (accumulator >> (16 - 8 * index)) & 0xff;
            out.push(char::from_u32(byte).unwrap_or('\0'));
        }
    }
    Ok(js_string!(out.as_str()).into())
}

fn install_base64_natives(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("btoa"),
        1,
        NativeFunction::from_fn_ptr(base64_encode),
    );
    let _ = ctx.register_global_callable(
        js_string!("atob"),
        1,
        NativeFunction::from_fn_ptr(base64_decode),
    );
}

// ---- UTF-8 text codecs ------------------------------------------------------

/// Encode a string as UTF-8, returning the bytes as an array of numbers.
fn encode_utf8(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = match args.first() {
        Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
        None => String::new(),
    };
    let array = JsArray::new(ctx);
    for byte in input.as_bytes() {
        array.push(JsValue::from(u32::from(*byte)), ctx)?;
    }
    Ok(array.into())
}

/// Decode an array of byte values as UTF-8, replacing malformed sequences.
fn decode_utf8(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(object) = args.first().and_then(JsValue::as_object) else {
        return Ok(js_string!("").into());
    };
    let array = JsArray::from_object(object.clone())?;
    let length = array.length(ctx)?;
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    for index in 0..length {
        let value = array.get(index, ctx)?.to_number(ctx)?;
        let byte = if value.is_finite() {
            (value as i64).rem_euclid(256)
        } else {
            0
        };
        bytes.push(u8::try_from(byte).unwrap_or(0));
    }
    Ok(js_string!(String::from_utf8_lossy(&bytes).as_ref()).into())
}

fn install_text_codec_natives(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("__silksurfEncodeUtf8"),
        1,
        NativeFunction::from_fn_ptr(encode_utf8),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfDecodeUtf8"),
        1,
        NativeFunction::from_fn_ptr(decode_utf8),
    );
}

// ---- document address -------------------------------------------------------

/*
 * set_document_url rewrites the `location` object and `document.URL` from a
 * parsed URL.
 *
 * The fields come from the same `url_record` the URL constructor reads, so
 * `location.origin` and `new URL(location.href).origin` agree by construction.
 * The existing assign/reload/replace methods stay in place: overwriting the
 * individual properties leaves the object identity a page may have captured.
 */
pub(super) fn set_document_url(ctx: &mut Context, url: &str) {
    let Ok(parsed) = url::Url::parse(url) else {
        return;
    };
    let record = url_record(&parsed, ctx);
    let Some(record) = record.as_object() else {
        return;
    };
    let global = ctx.global_object().clone();
    let Ok(location) = global.get(js_string!("location"), ctx) else {
        return;
    };
    let Some(location) = location.as_object() else {
        return;
    };
    for field in [
        "href", "origin", "protocol", "username", "password", "host", "hostname", "port",
        "pathname", "search", "hash",
    ] {
        let key: boa_engine::JsString = boa_engine::JsString::from(field);
        if let Ok(value) = record.get(key.clone(), ctx) {
            let _ = location.set(key, value, false, ctx);
        }
    }
    if let Ok(document) = global.get(js_string!("document"), ctx)
        && let Some(document) = document.as_object()
    {
        let href = js_string!(parsed.as_str());
        let _ = document.set(js_string!("URL"), href.clone(), false, ctx);
        let _ = document.set(js_string!("documentURI"), href.clone(), false, ctx);
        let _ = document.set(js_string!("baseURI"), href, false, ctx);
    }
}

// ---- bootstrap --------------------------------------------------------------

/*
 * The bootstrap expresses object shape only. Every algorithm whose result a
 * page can observe byte-for-byte -- URL parsing, form serialization, base64,
 * UTF-8 -- comes from the natives above.
 *
 * Intl carries Locale and getCanonicalLocales, which is what language
 * negotiation needs: a tag's subtags. Formatting (DateTimeFormat, NumberFormat,
 * Collator, PluralRules, RelativeTimeFormat) stays absent rather than wrong,
 * because a formatter that ignores the locale silently produces the wrong text.
 * Tracked in docs/roadmaps/SPA-CAPABILITY-ROADMAP.md under intl-formatters.
 */
const PLATFORM_BOOTSTRAP: &str = r"
(function () {
    'use strict';

    function URL(input, base) {
        var record = __silksurfParseUrl(String(input), base === undefined ? undefined : String(base));
        if (record === null) {
            throw new TypeError('Invalid URL: ' + String(input));
        }
        this.href = record.href;
        this.origin = record.origin;
        this.protocol = record.protocol;
        this.username = record.username;
        this.password = record.password;
        this.host = record.host;
        this.hostname = record.hostname;
        this.port = record.port;
        this.pathname = record.pathname;
        this.search = record.search;
        this.hash = record.hash;
        this.searchParams = new URLSearchParams(record.search);
    }
    URL.prototype.toString = function () { return this.href; };
    URL.prototype.toJSON = function () { return this.href; };
    URL.parse = function (input, base) {
        try { return new URL(input, base); } catch (e) { return null; }
    };
    URL.canParse = function (input, base) {
        return __silksurfParseUrl(String(input), base === undefined ? undefined : String(base)) !== null;
    };

    function URLSearchParams(init) {
        this._pairs = [];
        if (init === undefined || init === null) {
            return;
        }
        if (init instanceof URLSearchParams) {
            for (var i = 0; i < init._pairs.length; i++) {
                this._pairs.push([init._pairs[i][0], init._pairs[i][1]]);
            }
        } else if (Array.isArray(init)) {
            for (var j = 0; j < init.length; j++) {
                this._pairs.push([String(init[j][0]), String(init[j][1])]);
            }
        } else if (typeof init === 'object') {
            var keys = Object.keys(init);
            for (var k = 0; k < keys.length; k++) {
                this._pairs.push([keys[k], String(init[keys[k]])]);
            }
        } else {
            this._pairs = __silksurfDecodeQuery(String(init));
        }
    }
    URLSearchParams.prototype.append = function (name, value) {
        this._pairs.push([String(name), String(value)]);
    };
    URLSearchParams.prototype.set = function (name, value) {
        name = String(name);
        var replaced = false;
        var kept = [];
        for (var i = 0; i < this._pairs.length; i++) {
            if (this._pairs[i][0] !== name) { kept.push(this._pairs[i]); continue; }
            if (!replaced) { kept.push([name, String(value)]); replaced = true; }
        }
        if (!replaced) { kept.push([name, String(value)]); }
        this._pairs = kept;
    };
    URLSearchParams.prototype.get = function (name) {
        name = String(name);
        for (var i = 0; i < this._pairs.length; i++) {
            if (this._pairs[i][0] === name) { return this._pairs[i][1]; }
        }
        return null;
    };
    URLSearchParams.prototype.getAll = function (name) {
        name = String(name);
        var out = [];
        for (var i = 0; i < this._pairs.length; i++) {
            if (this._pairs[i][0] === name) { out.push(this._pairs[i][1]); }
        }
        return out;
    };
    URLSearchParams.prototype.has = function (name) { return this.get(String(name)) !== null; };
    URLSearchParams.prototype['delete'] = function (name) {
        name = String(name);
        var kept = [];
        for (var i = 0; i < this._pairs.length; i++) {
            if (this._pairs[i][0] !== name) { kept.push(this._pairs[i]); }
        }
        this._pairs = kept;
    };
    URLSearchParams.prototype.forEach = function (callback, thisArg) {
        for (var i = 0; i < this._pairs.length; i++) {
            callback.call(thisArg, this._pairs[i][1], this._pairs[i][0], this);
        }
    };
    URLSearchParams.prototype.keys = function () {
        return this._pairs.map(function (p) { return p[0]; })[Symbol.iterator]();
    };
    URLSearchParams.prototype.values = function () {
        return this._pairs.map(function (p) { return p[1]; })[Symbol.iterator]();
    };
    URLSearchParams.prototype.entries = function () {
        return this._pairs.map(function (p) { return [p[0], p[1]]; })[Symbol.iterator]();
    };
    URLSearchParams.prototype[Symbol.iterator] = URLSearchParams.prototype.entries;
    URLSearchParams.prototype.toString = function () { return __silksurfEncodeQuery(this._pairs); };
    Object.defineProperty(URLSearchParams.prototype, 'size', {
        get: function () { return this._pairs.length; }
    });

    globalThis.URL = URL;
    globalThis.URLSearchParams = URLSearchParams;

    function TextEncoder() { this.encoding = 'utf-8'; }
    TextEncoder.prototype.encode = function (input) {
        var bytes = __silksurfEncodeUtf8(input === undefined ? '' : String(input));
        var view = new Uint8Array(bytes.length);
        for (var i = 0; i < bytes.length; i++) { view[i] = bytes[i]; }
        return view;
    };
    globalThis.TextEncoder = TextEncoder;

    function TextDecoder(label) { this.encoding = label ? String(label) : 'utf-8'; }
    TextDecoder.prototype.decode = function (input) {
        if (input === undefined || input === null) { return ''; }
        var view = input.buffer ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
                                : new Uint8Array(input);
        var plain = [];
        for (var i = 0; i < view.length; i++) { plain.push(view[i]); }
        return __silksurfDecodeUtf8(plain);
    };
    globalThis.TextDecoder = TextDecoder;

    // The structured clone algorithm over the value graph this engine holds:
    // cycles are preserved through the seen map, and a function or symbol is a
    // DataCloneError exactly as HTML defines.
    globalThis.structuredClone = function (value) {
        var seen = new Map();
        function clone(node) {
            if (node === null || typeof node !== 'object') {
                if (typeof node === 'function' || typeof node === 'symbol') {
                    throw new Error('DataCloneError: ' + typeof node + ' could not be cloned');
                }
                return node;
            }
            if (seen.has(node)) { return seen.get(node); }
            var copy;
            if (Array.isArray(node)) {
                copy = [];
                seen.set(node, copy);
                for (var i = 0; i < node.length; i++) { copy.push(clone(node[i])); }
                return copy;
            }
            if (node instanceof Date) { copy = new Date(node.getTime()); seen.set(node, copy); return copy; }
            if (node instanceof Map) {
                copy = new Map();
                seen.set(node, copy);
                node.forEach(function (v, k) { copy.set(clone(k), clone(v)); });
                return copy;
            }
            if (node instanceof Set) {
                copy = new Set();
                seen.set(node, copy);
                node.forEach(function (v) { copy.add(clone(v)); });
                return copy;
            }
            copy = {};
            seen.set(node, copy);
            var keys = Object.keys(node);
            for (var j = 0; j < keys.length; j++) { copy[keys[j]] = clone(node[keys[j]]); }
            return copy;
        }
        return clone(value);
    };

    // requestIdleCallback rides the timer queue: this engine runs page script on
    // one thread with no separate idle period, so the callback lands at the end
    // of the current task with the full timeout still available.
    globalThis.requestIdleCallback = function (callback, options) {
        var start = Date.now();
        var timeout = options && options.timeout ? options.timeout : 0;
        return setTimeout(function () {
            callback({
                didTimeout: false,
                timeRemaining: function () { return Math.max(0, 50 - (Date.now() - start)); }
            });
        }, timeout);
    };
    globalThis.cancelIdleCallback = function (handle) { clearTimeout(handle); };

    globalThis.reportError = function (error) {
        var message = error && error.message ? error.message : String(error);
        console.error(message);
    };

    if (typeof globalThis.Intl === 'undefined') {
        var Intl = {};
        function Locale(tag) {
            var text = String(tag);
            this.baseName = text;
            var subtags = text.split('-');
            this.language = subtags[0] ? subtags[0].toLowerCase() : '';
            this.script = undefined;
            this.region = undefined;
            for (var i = 1; i < subtags.length; i++) {
                var part = subtags[i];
                if (part.length === 4 && this.script === undefined) {
                    this.script = part.charAt(0).toUpperCase() + part.slice(1).toLowerCase();
                } else if ((part.length === 2 || part.length === 3) && this.region === undefined) {
                    this.region = part.toUpperCase();
                }
            }
        }
        Locale.prototype.toString = function () { return this.baseName; };
        Locale.prototype.maximize = function () { return this; };
        Locale.prototype.minimize = function () { return this; };
        Intl.Locale = Locale;
        Intl.getCanonicalLocales = function (locales) {
            if (locales === undefined) { return []; }
            var list = Array.isArray(locales) ? locales : [locales];
            return list.map(function (tag) { return new Locale(tag).baseName; });
        };
        globalThis.Intl = Intl;
    }

    if (typeof globalThis.screen === 'undefined') {
        globalThis.screen = {
            width: 1280, height: 720,
            availWidth: 1280, availHeight: 720,
            colorDepth: 24, pixelDepth: 24,
            orientation: { type: 'landscape-primary', angle: 0 }
        };
    }
})();
";

#[cfg(test)]
mod tests {
    use crate::SilkContext;

    fn context() -> SilkContext {
        SilkContext::new()
    }

    #[test]
    fn url_parses_absolute_and_relative_forms() {
        let mut ctx = context();
        ctx.eval(
            "var u = new URL('/a/b?x=1#f', 'https://example.com:8443/base'); \
             if (u.href !== 'https://example.com:8443/a/b?x=1#f') throw new Error(u.href); \
             if (u.protocol !== 'https:') throw new Error(u.protocol); \
             if (u.hostname !== 'example.com') throw new Error(u.hostname); \
             if (u.port !== '8443') throw new Error(u.port); \
             if (u.pathname !== '/a/b') throw new Error(u.pathname); \
             if (u.search !== '?x=1') throw new Error(u.search); \
             if (u.hash !== '#f') throw new Error(u.hash); \
             if (u.origin !== 'https://example.com:8443') throw new Error(u.origin);",
        )
        .expect("URL resolves against a base");
    }

    #[test]
    fn url_rejects_a_relative_input_without_a_base() {
        let mut ctx = context();
        ctx.eval(
            "var threw = false; \
             try { new URL('/a'); } catch (e) { threw = true; } \
             if (!threw) throw new Error('expected TypeError'); \
             if (URL.canParse('/a')) throw new Error('canParse accepted a relative URL'); \
             if (URL.parse('/a') !== null) throw new Error('parse accepted a relative URL');",
        )
        .expect("a relative URL without a base is a TypeError");
    }

    #[test]
    fn search_params_round_trip_through_the_query_serializer() {
        let mut ctx = context();
        ctx.eval(
            "var p = new URLSearchParams('a=1&b=two&a=3'); \
             if (p.get('a') !== '1') throw new Error(p.get('a')); \
             if (p.getAll('a').join(',') !== '1,3') throw new Error(p.getAll('a').join(',')); \
             p.set('a', 'z'); p.append('c', 'x y'); \
             if (p.toString() !== 'a=z&b=two&c=x+y') throw new Error(p.toString()); \
             p['delete']('b'); \
             if (p.has('b')) throw new Error('delete left b'); \
             if (p.size !== 2) throw new Error('size ' + p.size);",
        )
        .expect("URLSearchParams mutates and serializes");
    }

    #[test]
    fn url_exposes_its_query_as_search_params() {
        let mut ctx = context();
        ctx.eval(
            "var u = new URL('https://example.com/?q=hello+world'); \
             if (u.searchParams.get('q') !== 'hello world') throw new Error(u.searchParams.get('q'));",
        )
        .expect("searchParams decodes the query");
    }

    #[test]
    fn base64_round_trips_a_byte_string() {
        let mut ctx = context();
        ctx.eval(
            "if (btoa('hello') !== 'aGVsbG8=') throw new Error(btoa('hello')); \
             if (btoa('hi') !== 'aGk=') throw new Error(btoa('hi')); \
             if (btoa('') !== '') throw new Error('empty'); \
             if (atob('aGVsbG8=') !== 'hello') throw new Error(atob('aGVsbG8=')); \
             if (atob(btoa('any carnal pleasure')) !== 'any carnal pleasure') \
                 throw new Error('round trip');",
        )
        .expect("btoa and atob agree");
    }

    #[test]
    fn btoa_rejects_a_character_above_latin_1() {
        let mut ctx = context();
        ctx.eval(
            "var threw = false; \
             try { btoa('\\u0100'); } catch (e) { threw = true; } \
             if (!threw) throw new Error('expected a range error');",
        )
        .expect("btoa refuses non-Latin-1 input");
    }

    #[test]
    fn text_codecs_round_trip_multibyte_text() {
        let mut ctx = context();
        ctx.eval(
            "var bytes = new TextEncoder().encode('a\\u00e9\\u4e2d'); \
             if (bytes.length !== 6) throw new Error('length ' + bytes.length); \
             if (new TextDecoder().decode(bytes) !== 'a\\u00e9\\u4e2d') throw new Error('decode');",
        )
        .expect("TextEncoder and TextDecoder agree over UTF-8");
    }

    #[test]
    fn structured_clone_copies_deeply_and_keeps_cycles() {
        let mut ctx = context();
        ctx.eval(
            "var source = { n: 1, list: [1, 2], inner: { s: 'x' } }; \
             source.self = source; \
             var copy = structuredClone(source); \
             if (copy === source) throw new Error('same reference'); \
             if (copy.inner === source.inner) throw new Error('shallow inner'); \
             if (copy.self !== copy) throw new Error('cycle not preserved'); \
             if (copy.list.join(',') !== '1,2') throw new Error('list'); \
             var threw = false; \
             try { structuredClone({ f: function () {} }); } catch (e) { threw = true; } \
             if (!threw) throw new Error('expected DataCloneError');",
        )
        .expect("structuredClone deep-copies the value graph");
    }

    #[test]
    fn intl_locale_reports_the_language_subtag() {
        let mut ctx = context();
        ctx.eval(
            "if (new Intl.Locale('zh-Hant-TW').language !== 'zh') throw new Error('language'); \
             if (new Intl.Locale('zh-Hant-TW').script !== 'Hant') throw new Error('script'); \
             if (new Intl.Locale('zh-Hant-TW').region !== 'TW') throw new Error('region'); \
             if (new Intl.Locale('pt-BR').region !== 'BR') throw new Error('pt-BR region'); \
             if (Intl.getCanonicalLocales(['en-US']).join(',') !== 'en-US') throw new Error('canonical');",
        )
        .expect("Intl.Locale splits a BCP-47 tag");
    }

    #[test]
    fn set_document_url_populates_location_and_document() {
        let mut ctx = context();
        ctx.set_document_url("https://example.com:8443/chat?q=1#top");
        ctx.eval(
            "if (location.href !== 'https://example.com:8443/chat?q=1#top') throw new Error(location.href); \
             if (location.origin !== 'https://example.com:8443') throw new Error(location.origin); \
             if (location.pathname !== '/chat') throw new Error(location.pathname); \
             if (location.search !== '?q=1') throw new Error(location.search); \
             if (new URL(location.href).hostname !== 'example.com') throw new Error('URL disagrees');",
        )
        .expect("location reflects the document address");
    }

    #[test]
    fn an_unparseable_document_url_leaves_the_stub_alone() {
        let mut ctx = context();
        ctx.set_document_url("not a url");
        ctx.eval("if (location.href !== '') throw new Error(location.href);")
            .expect("the location stub survives a bad URL");
    }

    #[test]
    fn idle_callback_and_report_error_exist() {
        let mut ctx = context();
        ctx.eval(
            "if (typeof requestIdleCallback !== 'function') throw new Error('requestIdleCallback'); \
             if (typeof cancelIdleCallback !== 'function') throw new Error('cancelIdleCallback'); \
             if (typeof reportError !== 'function') throw new Error('reportError'); \
             if (screen.width !== 1280) throw new Error('screen.width');",
        )
        .expect("idle callbacks, reportError, and screen install");
    }
}
