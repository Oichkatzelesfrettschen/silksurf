use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

#[test]
fn basic_client_errors_without_transport() {
    let client = BasicClient::new();
    let request = HttpRequest {
        method: HttpMethod::Get,
        url: "https://example.com".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let err = client.fetch(&request).expect_err("not implemented");
    assert!(err.message.contains("not implemented"));
}
