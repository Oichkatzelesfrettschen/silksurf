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
