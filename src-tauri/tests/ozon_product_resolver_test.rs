use desktop_app_lib::core::ozon_product::{
    classify_ozon_url_mode,
    resolve_ozon_product,
    OzonProductResolution,
    OzonResolutionFailure,
};
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn ozon_product_resolver_symbols_are_available() {
    let _ = classify_ozon_url_mode as fn(&str) -> bool;
    let _ = resolve_ozon_product;
    let _ = std::mem::size_of::<OzonProductResolution>();
    let _ = std::mem::size_of::<OzonResolutionFailure>();
}

#[derive(Clone)]
struct HttpResponse {
    status_line: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

fn png_bytes(fill: u8) -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, fill, 0, 0,
        0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]
}

fn spawn_fixture_server(
    routes: HashMap<String, HttpResponse>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture listener");
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve fixture listener address");
    let routes = Arc::new(routes);

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let mut last_activity = Instant::now();
        let mut served_any = false;

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let response = routes.get(path).cloned().unwrap_or(HttpResponse {
                        status_line: "404 Not Found",
                        content_type: "text/plain; charset=utf-8",
                        body: b"not found".to_vec(),
                    });

                    let headers = format!(
                        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status_line,
                        response.content_type,
                        response.body.len()
                    );
                    let _ = stream.write_all(headers.as_bytes());
                    let _ = stream.write_all(&response.body);
                    served_any = true;
                    last_activity = Instant::now();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if served_any && last_activity.elapsed() >= Duration::from_millis(250) {
                        return;
                    }
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}"), handle)
}

fn structured_product_html(title: &str, image_urls: &[&str]) -> String {
    let images_json = image_urls
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta property="og:title" content="{title}">
    <meta property="og:image" content="/images/fallback.png">
    <script type="application/ld+json">
      {{
        "@context":"https://schema.org",
        "@type":"Product",
        "name":"{title}",
        "image":[{images_json}]
      }}
    </script>
  </head>
  <body><h1>{title}</h1></body>
</html>"#
    )
}

#[test]
fn classify_ozon_url_mode_accepts_ozon_product_urls() {
    assert!(classify_ozon_url_mode(
        "https://www.ozon.ru/product/3570411009"
    ));
    assert!(classify_ozon_url_mode("https://ozon.ru/product/3570411009/"));
    assert!(!classify_ozon_url_mode(
        "https://www.ozon.ru/category/3570411009"
    ));
    assert!(!classify_ozon_url_mode(
        "https://example.com/product/3570411009"
    ));
}

#[test]
fn resolve_ozon_product_extracts_title_and_first_main_image_from_structured_html() {
    let client = Client::new();
    let first_image = png_bytes(248);
    let second_image = png_bytes(128);
    let mut routes = HashMap::new();
    routes.insert(
        "/images/main-1.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: first_image.clone(),
        },
    );
    routes.insert(
        "/images/main-2.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: second_image,
        },
    );
    routes.insert(
        "/images/fallback.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: png_bytes(64),
        },
    );
    routes.insert(
        "/product/3570411009".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: structured_product_html(
                "Морская верёвочная лестница",
                &["/images/main-1.png", "/images/main-2.png"],
            )
            .into_bytes(),
        },
    );
    let (base_url, handle) = spawn_fixture_server(routes);

    let resolution =
        resolve_ozon_product(&client, &format!("{base_url}/product/3570411009")).expect(
            "resolver should extract title and first image",
        );

    assert_eq!(resolution.title, "Морская верёвочная лестница");
    assert_eq!(resolution.image_url, format!("{base_url}/images/main-1.png"));
    assert_eq!(resolution.image_bytes, first_image);

    handle.join().expect("join fixture server");
}

#[test]
fn resolve_ozon_product_prefers_first_image_only() {
    let client = Client::new();
    let first_image = png_bytes(230);
    let second_image = png_bytes(12);
    let mut routes = HashMap::new();
    routes.insert(
        "/images/first.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: first_image.clone(),
        },
    );
    routes.insert(
        "/images/second.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: second_image,
        },
    );
    routes.insert(
        "/product/3570411009".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: structured_product_html("Товар для теста", &["/images/first.png", "/images/second.png"])
                .into_bytes(),
        },
    );
    let (base_url, handle) = spawn_fixture_server(routes);

    let resolution =
        resolve_ozon_product(&client, &format!("{base_url}/product/3570411009")).expect(
            "resolver should prefer first image",
        );

    assert_eq!(resolution.image_url, format!("{base_url}/images/first.png"));
    assert_eq!(resolution.image_bytes, first_image);

    handle.join().expect("join fixture server");
}

#[test]
fn resolve_ozon_product_returns_unavailable_for_off_shelf_html() {
    let client = Client::new();
    let mut routes = HashMap::new();
    routes.insert(
        "/product/3570411009".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: r#"<html><body><h1>Такого товара нет</h1></body></html>"#
                .as_bytes()
                .to_vec(),
        },
    );
    let (base_url, handle) = spawn_fixture_server(routes);

    let error = resolve_ozon_product(&client, &format!("{base_url}/product/3570411009"))
        .expect_err("off-shelf product should be classified as unavailable");

    assert_eq!(error, OzonResolutionFailure::Unavailable);
    handle.join().expect("join fixture server");
}

#[test]
fn resolve_ozon_product_returns_missing_title_when_title_is_absent() {
    let client = Client::new();
    let mut routes = HashMap::new();
    routes.insert(
        "/images/main.png".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "image/png",
            body: png_bytes(188),
        },
    );
    routes.insert(
        "/product/3570411009".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: format!(
                r#"<html><head><script type="application/ld+json">{{"@context":"https://schema.org","@type":"Product","image":["/images/main.png"]}}</script></head></html>"#
            )
            .into_bytes(),
        },
    );
    let (base_url, handle) = spawn_fixture_server(routes);

    let error = resolve_ozon_product(&client, &format!("{base_url}/product/3570411009"))
        .expect_err("missing title should be classified");

    assert_eq!(error, OzonResolutionFailure::MissingTitle);
    handle.join().expect("join fixture server");
}

#[test]
fn resolve_ozon_product_returns_missing_image_when_image_is_absent() {
    let client = Client::new();
    let mut routes = HashMap::new();
    routes.insert(
        "/product/3570411009".to_string(),
        HttpResponse {
            status_line: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: r#"<html><head><script type="application/ld+json">{"@context":"https://schema.org","@type":"Product","name":"无主图测试"}</script></head></html>"#
                .as_bytes()
                .to_vec(),
        },
    );
    let (base_url, handle) = spawn_fixture_server(routes);

    let error = resolve_ozon_product(&client, &format!("{base_url}/product/3570411009"))
        .expect_err("missing image should be classified");

    assert_eq!(error, OzonResolutionFailure::MissingImage);
    handle.join().expect("join fixture server");
}

#[test]
fn resolve_ozon_product_returns_invalid_url_for_unsupported_links() {
    let client = Client::new();
    let error = resolve_ozon_product(&client, "https://example.com/product/3570411009")
        .expect_err("unsupported url should be rejected");
    assert_eq!(error, OzonResolutionFailure::InvalidUrl);
}
