# Cloudflare Turnstile Redirect and Frame Gate

**Date**: 2026-09-24
**Mechanism**: same-host HTTP/2 redirects continue through the bounded HTTP/1.1 redirect loop; iframe documents run in independent runtimes and composite into owner boxes. The Turnstile API creates a closed-shadow iframe and exchanges lifecycle and `postMessage` events before Cloudflare rejects SilkSurf as an unsupported browser.
**Question**: can SilkSurf load Cloudflare's interactive Turnstile test widget and expose a real click target through its embedded-content path?

## Verdict

The redirect defect is repaired by continuing an HTTP/2 redirect response through the existing bounded HTTP/1.1 loop. The continuation starts at the response `Location`, so the initial GET is not replayed. The HTTP/2 path now sends and stores subresource cookies and routes origin-restricted batches through the HTTP/1.1 origin check.

The official Cloudflare test key `3x00000000000000000000FF` loads the 86,732-byte Turnstile API and creates a challenge iframe inside a closed shadow root. SilkSurf fetches and executes the child document, dispatches document lifecycle events, and completes the `init`, `requestExtraParams`, `extraParams`, and `execute` message exchange. The challenge then sends `reject` with reason `unsupported_browser`. The native screenshot displays Cloudflare's browser-not-supported panel without a checkbox.

![SilkSurf renders Cloudflare's browser-not-supported panel without a checkbox](images/cloudflare-turnstile-unsupported-browser.png)

Chromium 153.0.8010.36 on the same local fixture and test key renders Cloudflare's visible `Verify you are human` checkbox. This control confirms that the key, fixture origin, and network path produce an interactive widget in a supported browser. SilkSurf's live `unsupported_browser` response remains tied to its runtime or browser identity; the challenge does not expose the exact rejection predicate.

![Chromium renders the visible Turnstile test checkbox on the same fixture](images/cloudflare-turnstile-chromium-test-key.png)

The Turnstile API reads `window.innerWidth` and `window.innerHeight` when it builds challenge parameters. SilkSurf updates both CSSOM View properties from each document's viewport, and a runtime test covers initial dimensions and resize. A separate iframe sizing defect applied the HTML intrinsic 300-by-150 ratio after CSS specified 300-by-65. The layout now applies the intrinsic ratio only while a computed dimension remains `auto`, and the focused test verifies the resulting 300-by-65 owner box.

## Fresh runtime trace

A fresh X11 run after the layout correction records the parent iframe owner and child runtime at 300-by-65. The child exchanges `init`, `requestExtraParams`, `extraParams`, and `execute`, then emits repeated `meow` messages and `forceFail`. The API reports Turnstile error `300030` with `native construct`. The child navigation also reports a `SyntaxError` at `let` while evaluating script 0, then changes the iframe URL to `crashed_retry`. The current run therefore fails through the child runtime path before it reproduces the earlier `reject` message with reason `unsupported_browser`.

A subsequent fresh X11 run begins with a 300-by-150 child viewport, applies the explicit 300-by-65 iframe size before `execute`, and receives `reject reason=unsupported_browser`. The parent API's rejection handler takes the unsupported callback branch for that reason. The captured window shows the browser-not-supported panel and no checkbox. This run reproduces the admission rejection; it does not reproduce the earlier child-script `SyntaxError` and `forceFail` path.

Fresh Chromium 153 on the same fixture visibly renders the official test checkbox. Its parent document reports `navigator.userAgent` as a Chromium 153 identity, `navigator.sendBeacon` as a function, and `document.featurePolicy` as an object. The returned feature list excludes `private-token`. SilkSurf reports its truthful `SilkSurf/0.1` user agent, no `navigator.sendBeacon`, and no `document.featurePolicy`. The fetched API source uses `document.featurePolicy.features()` only to add the optional `private-token` iframe permission; Chromium and SilkSurf both omit that permission. The API uses `sendBeacon` only for automatic feedback reporting and falls back to `fetch` when the method is unavailable. These measured differences do not establish the cause of Cloudflare's earlier `unsupported_browser` response.

The API's `reject` handler compares the received message's `reason` with the string `unsupported_browser` and calls the widget's unsupported callback. The challenge child supplies that reason; the parent handler reads no browser identity or capability value on this branch. SilkSurf reports `navigator.userAgent=SilkSurf/0.1` and sends `SilkSurf/0.1 (X11; Linux x86_64)`, while Chromium reports a `Chrome/153.0.0.0` user agent. Fresh A/B runs changed the request header to Chromium's value, then changed both the request header and `navigator.userAgent`; both runs still received `reject reason=unsupported_browser`. The UA difference is a demonstrated compatibility gap, while the A/B result shows that UA identity alone does not explain this rejection. The challenge child's browser-admission predicate remains opaque in the live evidence. The earlier `forceFail` run exposes a separate runtime incompatibility; the `let` diagnostic does not yet identify the dynamically evaluated source or the browser property that selects it.

## Evidence and reproduction

Cloudflare documents the test key as forcing an interactive visible widget in [Test your Turnstile implementation](https://developers.cloudflare.com/turnstile/troubleshooting/testing/). The native X11 run loads that key through `https://challenges.cloudflare.com/turnstile/v0/api.js`, fetches the iframe document, and records the child-to-parent handshake through `execute`. Cloudflare then returns `reject` with `reason=unsupported_browser`; the screenshot captures the rendered rejection panel and the absent checkbox.

Focused tests cover document lifecycle ordering, live `NamedNodeMap` attributes, `NodeIterator` traversal, the live `document.scripts` collection, and explicit iframe dimensions. `make full` passes on the pushed child-frame branch. Child-frame navigation, 300-by-65 composition, message delivery, and the Chromium checkbox have live evidence; SilkSurf checkbox rendering remains unaccepted.

## Residual

Cloudflare's exact browser-admission predicate remains unobserved. Fresh SilkSurf runs separately produce `reject reason=unsupported_browser` and a child-script runtime error followed by `forceFail`. The next decisive trace captures the dynamically evaluated source and the challenge child's browser-property reads in SilkSurf and Chromium. Child input routing remains unverified because SilkSurf does not yet paint the checkbox.
