use silksurf_net::{BasicClient, HttpMethod, HttpRequest};
use std::time::Instant;

fn main() {
    let urls: Vec<String> = std::env::args().skip(1).collect();
    if urls.is_empty() {
        eprintln!("Usage: h2-fetch-probe <url> [url ...]");
        std::process::exit(1);
    }

    let requests: Vec<HttpRequest> = urls.iter().map(|url| request_for_url(url)).collect();
    let client = BasicClient::new();
    let started = Instant::now();
    let responses = client.fetch_parallel(&requests);
    let elapsed = started.elapsed();

    println!(
        "h2-fetch-probe: {} request(s) in {:?}",
        responses.len(),
        elapsed
    );

    let mut failed = false;
    for (index, (url, result)) in urls.iter().zip(responses.iter()).enumerate() {
        match result {
            Ok(response) => {
                println!(
                    "{index}: {} {} bytes {}",
                    response.status,
                    response.body.len(),
                    url
                );
            }
            Err(error) => {
                failed = true;
                println!("{index}: error {} {}", error.message, url);
            }
        }
    }

    if failed {
        std::process::exit(2);
    }
}

fn request_for_url(url: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: url.to_string(),
        headers: vec![
            ("Accept".to_string(), "text/html,text/css,*/*".to_string()),
            (
                "User-Agent".to_string(),
                "SilkSurf/0.1 (X11; Linux x86_64)".to_string(),
            ),
        ],
        body: Vec::new(),
    }
}
