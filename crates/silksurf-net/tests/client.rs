use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

#[test]
fn basic_client_returns_response_or_network_error() {
    let client = BasicClient::new();
    let request = HttpRequest {
        method: HttpMethod::Get,
        url: "https://example.com".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    // Should either succeed with a response or fail with a network error.
    // Will not panic or return "not implemented" -- the client is real now.
    match client.fetch(&request) {
        Ok(response) => {
            assert!(response.status > 0);
            assert!(!response.headers.is_empty());
        }
        Err(err) => {
            // Network errors are expected in CI/sandboxed environments
            assert!(!err.message.is_empty());
        }
    }
}

#[test]
fn basic_client_invalid_url() {
    let client = BasicClient::new();
    let request = HttpRequest {
        method: HttpMethod::Get,
        url: "not-a-url".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let err = client.fetch(&request).expect_err("should fail");
    assert!(err.message.contains("Invalid URL"));
}

#[test]
fn basic_client_response_header_lookup() {
    use silksurf_net::HttpResponse;
    let response = HttpResponse {
        status: 200,
        headers: vec![
            ("Content-Type".to_string(), "text/html".to_string()),
            ("X-Custom".to_string(), "value".to_string()),
        ],
        body: vec![],
    };
    assert_eq!(response.header("content-type"), Some("text/html"));
    assert_eq!(response.header("X-CUSTOM"), Some("value"));
    assert_eq!(response.header("missing"), None);
}

#[test]
fn fetch_parallel_malformed_url_yields_per_request_errors() {
    /*
     * A malformed URL in a batch must surface as that request's own error.
     * The h2 fast path substituted "https://localhost/" for unparseable
     * URLs; the parse-once restructure sends such batches down the
     * HTTP/1.1 path where fetch() fails at URL parse -- before any
     * socket I/O, so this test never touches the network.
     */
    let client = BasicClient::new();
    let make = |url: &str| HttpRequest {
        method: HttpMethod::Get,
        url: url.into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let results = client.fetch_parallel(&[make("not a url"), make("::also::bad::")]);
    assert_eq!(results.len(), 2);
    for result in results {
        let err = result.expect_err("malformed URL must error, not be rewritten");
        assert!(!err.message.is_empty());
    }
}
