use crate::config::{LocationConfig, LocationHandler, LoomisConfig, ServerConfig};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug)]
pub enum ServerError {
    InvalidRootDirectory(PathBuf),
    Io(io::Error),
    ThreadPanic,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRootDirectory(path) => {
                write!(f, "invalid html root directory: {}", path.display())
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::ThreadPanic => write!(f, "listener thread panicked"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<io::Error> for ServerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn serve_config(config: &LoomisConfig) -> Result<(), ServerError> {
    let listeners = group_servers_by_listener(config);
    let mut handles = Vec::with_capacity(listeners.len());

    for listener in listeners {
        handles.push(thread::spawn(move || serve_listener(listener)));
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(ServerError::ThreadPanic),
        }
    }

    Ok(())
}

pub fn serve_html(root_dir: impl AsRef<Path>, port: u16) -> Result<(), ServerError> {
    let root_dir = canonicalize_root(root_dir.as_ref())?;
    let config = LoomisConfig {
        servers: vec![ServerConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], port)),
            server_names: vec![String::from("localhost")],
            locations: vec![LocationConfig {
                path: String::from("/"),
                handler: LocationHandler::Static { root: root_dir },
                index: vec![String::from("index.html")],
            }],
        }],
    };

    serve_config(&config)
}

fn serve_listener(listener_config: ListenerConfig) -> Result<(), ServerError> {
    let listener = TcpListener::bind(listener_config.listen)?;
    println!(
        "Listening on http://{}/ with {} server block(s)",
        listener_config.listen,
        listener_config.servers.len()
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let servers = listener_config.servers.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &servers) {
                        eprintln!("connection error: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept error: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, servers: &[ServerConfig]) -> io::Result<()> {
    let request = match read_http_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(RequestReadError::Io(error)) => return Err(error),
        Err(RequestReadError::Malformed(message)) => {
            return write_text_response(
                &mut stream,
                "400 Bad Request",
                &message,
                false,
                false,
            );
        }
    };

    let server = select_server(servers, request.host());
    let Some(location) = select_location(&server.locations, &request.path) else {
        return write_text_response(
            &mut stream,
            "404 Not Found",
            "No matching location block.",
            request.is_head(),
            false,
        );
    };

    match &location.handler {
        LocationHandler::Static { root } => {
            serve_static_location(&mut stream, &request, location, root)
        }
        LocationHandler::Proxy { .. } => write_text_response(
            &mut stream,
            "501 Not Implemented",
            "proxy_pass support is not available yet.",
            request.is_head(),
            false,
        ),
    }
}

fn serve_static_location(
    stream: &mut TcpStream,
    request: &HttpRequest,
    location: &LocationConfig,
    root: &Path,
) -> io::Result<()> {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return write_text_response(
            stream,
            "405 Method Not Allowed",
            "Only GET and HEAD requests are supported for static locations.",
            request.is_head(),
            true,
        );
    }

    match resolve_static_file(root, &location.path, &request.path, &location.index) {
        Ok(Some(file_path)) => {
            let content_type = content_type_for_path(&file_path);
            let body = if request.is_head() {
                Vec::new()
            } else {
                fs::read(&file_path)?
            };
            let content_length = if request.is_head() {
                fs::metadata(&file_path)?.len()
            } else {
                body.len() as u64
            };

            write_response(
                stream,
                "200 OK",
                &[(String::from("Content-Type"), content_type.to_string())],
                Some(&body),
                content_length,
                request.is_head(),
            )
        }
        Ok(None) => write_text_response(
            stream,
            "404 Not Found",
            "Static file not found.",
            request.is_head(),
            false,
        ),
        Err(PathResolutionError::TraversalAttempt) => write_text_response(
            stream,
            "400 Bad Request",
            "Invalid request path.",
            request.is_head(),
            false,
        ),
    }
}

fn group_servers_by_listener(config: &LoomisConfig) -> Vec<ListenerConfig> {
    let mut grouped: HashMap<SocketAddr, Vec<ServerConfig>> = HashMap::new();
    for server in &config.servers {
        grouped
            .entry(server.listen)
            .or_default()
            .push(server.clone());
    }

    let mut listeners = grouped
        .into_iter()
        .map(|(listen, servers)| ListenerConfig { listen, servers })
        .collect::<Vec<_>>();
    listeners.sort_by_key(|listener| listener.listen);
    listeners
}

fn select_server<'a>(servers: &'a [ServerConfig], host: Option<&str>) -> &'a ServerConfig {
    let normalized_host = normalize_host(host);

    if let Some(host) = normalized_host.as_deref() {
        if let Some(server) = servers
            .iter()
            .find(|server| server.server_names.iter().any(|name| name == host))
        {
            return server;
        }
    }

    servers
        .iter()
        .find(|server| server.server_names.is_empty())
        .unwrap_or(&servers[0])
}

fn select_location<'a>(
    locations: &'a [LocationConfig],
    request_path: &str,
) -> Option<&'a LocationConfig> {
    locations
        .iter()
        .filter(|location| location_matches_request(&location.path, request_path))
        .max_by_key(|location| location.path.len())
}

fn location_matches_request(location_path: &str, request_path: &str) -> bool {
    location_path == "/"
        || request_path == location_path
        || request_path
            .strip_prefix(location_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn resolve_static_file(
    root_dir: &Path,
    location_path: &str,
    request_path: &str,
    index_files: &[String],
) -> Result<Option<PathBuf>, PathResolutionError> {
    let suffix = strip_location_prefix(location_path, request_path);
    let sanitized_segments = sanitize_segments(suffix)?;
    let mut base_path = root_dir.to_path_buf();

    for segment in &sanitized_segments {
        base_path.push(segment);
    }

    let mut candidates = Vec::new();
    if sanitized_segments.is_empty() || request_path.ends_with('/') {
        for index in index_files {
            candidates.push(base_path.join(index));
        }
    } else {
        candidates.push(base_path.clone());
        if base_path.extension().is_none() {
            candidates.push(base_path.with_extension("html"));
            for index in index_files {
                candidates.push(base_path.join(index));
            }
        }
    }

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }

        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.starts_with(root_dir) {
                return Ok(Some(canonical));
            }
        }
    }

    Ok(None)
}

fn strip_location_prefix<'a>(location_path: &str, request_path: &'a str) -> &'a str {
    if location_path == "/" {
        return request_path;
    }

    request_path
        .strip_prefix(location_path)
        .filter(|suffix| !suffix.is_empty())
        .unwrap_or("/")
}

fn sanitize_segments(request_path: &str) -> Result<Vec<&str>, PathResolutionError> {
    let trimmed = request_path.trim_start_matches('/');

    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }

        if segment == ".." || segment.contains('\\') {
            return Err(PathResolutionError::TraversalAttempt);
        }

        segments.push(segment);
    }

    Ok(segments)
}

fn canonicalize_root(root_dir: &Path) -> Result<PathBuf, ServerError> {
    let canonical = root_dir
        .canonicalize()
        .map_err(|_| ServerError::InvalidRootDirectory(root_dir.to_path_buf()))?;

    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(ServerError::InvalidRootDirectory(root_dir.to_path_buf()))
    }
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("css") => "text/css; charset=utf-8",
        Some("gif") => "image/gif",
        Some("htm") | Some("html") => "text/html; charset=utf-8",
        Some("ico") => "image/x-icon",
        Some("jpeg") | Some("jpg") => "image/jpeg",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("xml") => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>, RequestReadError> {
    let mut buffer = Vec::with_capacity(8 * 1024);
    let mut chunk = [0_u8; 4 * 1024];

    loop {
        let read_len = stream.read(&mut chunk).map_err(RequestReadError::Io)?;
        if read_len == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }

            return Err(RequestReadError::Malformed(String::from(
                "Unexpected EOF while reading the HTTP request.",
            )));
        }

        buffer.extend_from_slice(&chunk[..read_len]);
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Err(RequestReadError::Malformed(String::from(
                "HTTP request headers are too large.",
            )));
        }

        if let Some(header_end) = find_header_end(&buffer) {
            let head = parse_request_head(&buffer[..header_end])?;
            let content_length = parse_content_length(&head.headers)?;
            if content_length > MAX_REQUEST_BODY_BYTES {
                return Err(RequestReadError::Malformed(String::from(
                    "HTTP request body is too large.",
                )));
            }

            let body_start = header_end + 4;
            let mut body = buffer[body_start..].to_vec();
            while body.len() < content_length {
                let read_len = stream.read(&mut chunk).map_err(RequestReadError::Io)?;
                if read_len == 0 {
                    return Err(RequestReadError::Malformed(String::from(
                        "Unexpected EOF while reading the HTTP request body.",
                    )));
                }
                body.extend_from_slice(&chunk[..read_len]);
            }
            body.truncate(content_length);

            return Ok(Some(HttpRequest { body, ..head }));
        }
    }
}

fn parse_request_head(buffer: &[u8]) -> Result<HttpRequest, RequestReadError> {
    let request = std::str::from_utf8(buffer).map_err(|_| {
        RequestReadError::Malformed(String::from("HTTP request headers must be valid UTF-8."))
    })?;
    let mut lines = request.split("\r\n");
    let request_line = lines.next().ok_or_else(|| {
        RequestReadError::Malformed(String::from("Malformed HTTP request line."))
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| RequestReadError::Malformed(String::from("Missing HTTP method.")))?;
    let target = parts
        .next()
        .ok_or_else(|| RequestReadError::Malformed(String::from("Missing HTTP request target.")))?;
    let version = parts
        .next()
        .ok_or_else(|| RequestReadError::Malformed(String::from("Missing HTTP version.")))?;

    if parts.next().is_some() {
        return Err(RequestReadError::Malformed(String::from(
            "Malformed HTTP request line.",
        )));
    }

    let (path, query) = parse_request_target(target)?;
    let mut headers = Vec::new();

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let Some((name, value)) = line.split_once(':') else {
            return Err(RequestReadError::Malformed(String::from(
                "Malformed HTTP header.",
            )));
        };

        headers.push(HttpHeader {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
        });
    }

    Ok(HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
        path,
        query,
        version: version.to_string(),
        headers,
        body: Vec::new(),
    })
}

fn parse_request_target(target: &str) -> Result<(String, Option<String>), RequestReadError> {
    let without_fragment = target.split('#').next().unwrap_or(target);
    let (path, query) = match without_fragment.split_once('?') {
        Some((path, query)) => (path, Some(query.to_string())),
        None => (without_fragment, None),
    };

    if !path.starts_with('/') {
        return Err(RequestReadError::Malformed(String::from(
            "Only origin-form request targets are supported.",
        )));
    }

    Ok((path.to_string(), query))
}

fn parse_content_length(headers: &[HttpHeader]) -> Result<usize, RequestReadError> {
    let Some(value) = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-length"))
        .map(|header| header.value.as_str())
    else {
        return Ok(0);
    };

    value.parse::<usize>().map_err(|_| {
        RequestReadError::Malformed(String::from(
            "Content-Length must be a positive integer.",
        ))
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_text_response(
    stream: &mut TcpStream,
    status: &str,
    message: &str,
    head_only: bool,
    include_allow_header: bool,
) -> io::Result<()> {
    let mut headers = vec![(
        String::from("Content-Type"),
        String::from("text/plain; charset=utf-8"),
    )];
    if include_allow_header {
        headers.push((String::from("Allow"), String::from("GET, HEAD")));
    }

    write_response(
        stream,
        status,
        &headers,
        Some(message.as_bytes()),
        message.len() as u64,
        head_only,
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    content_length: u64,
    head_only: bool,
) -> io::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {content_length}\r\nConnection: close\r\n"
    );
    for (name, value) in headers {
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        header.push_str("\r\n");
    }
    header.push_str("\r\n");

    stream.write_all(header.as_bytes())?;
    if !head_only {
        if let Some(body) = body {
            stream.write_all(body)?;
        }
    }
    stream.flush()
}

fn normalize_host(host: Option<&str>) -> Option<String> {
    let host = host?.trim();
    if host.is_empty() {
        return None;
    }

    if host.starts_with('[') {
        return host
            .split(']')
            .next()
            .map(|value| value.trim_start_matches('[').to_ascii_lowercase());
    }

    let authority = host.split_once(':').map(|(name, _)| name).unwrap_or(host);
    Some(authority.to_ascii_lowercase())
}

#[derive(Debug, Clone)]
struct ListenerConfig {
    listen: SocketAddr,
    servers: Vec<ServerConfig>,
}

#[derive(Debug)]
enum PathResolutionError {
    TraversalAttempt,
}

#[derive(Debug)]
enum RequestReadError {
    Io(io::Error),
    Malformed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    target: String,
    path: String,
    query: Option<String>,
    version: String,
    headers: Vec<HttpHeader>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn host(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("host"))
            .map(|header| header.value.as_str())
    }

    fn is_head(&self) -> bool {
        self.method.eq_ignore_ascii_case("HEAD")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpHeader {
    name: String,
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProxyTarget;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn selects_named_server_by_host_header() {
        let servers = vec![
            build_server(
                "127.0.0.1:3000",
                vec![String::from("default.test")],
                vec![build_static_location("/")],
            ),
            build_server(
                "127.0.0.1:3000",
                vec![String::from("api.test")],
                vec![build_static_location("/")],
            ),
        ];

        let matched = select_server(&servers, Some("api.test:3000"));

        assert_eq!(matched.server_names, vec![String::from("api.test")]);
    }

    #[test]
    fn falls_back_to_default_server_when_host_is_missing() {
        let servers = vec![
            build_server(
                "127.0.0.1:3000",
                Vec::new(),
                vec![build_static_location("/")],
            ),
            build_server(
                "127.0.0.1:3000",
                vec![String::from("api.test")],
                vec![build_static_location("/")],
            ),
        ];

        let matched = select_server(&servers, None);

        assert!(matched.server_names.is_empty());
    }

    #[test]
    fn chooses_longest_matching_location_prefix() {
        let locations = vec![
            build_static_location("/"),
            build_static_location("/assets"),
            build_static_location("/assets/admin"),
        ];

        let matched =
            select_location(&locations, "/assets/admin/logo.png").expect("location should match");

        assert_eq!(matched.path, "/assets/admin");
    }

    #[test]
    fn resolves_file_relative_to_location_prefix() {
        let root = create_test_site();
        let static_root = root.join("assets");
        fs::create_dir_all(&static_root).expect("failed to create assets dir");
        fs::write(static_root.join("app.css"), "body{}").expect("failed to write asset");

        let resolved = resolve_static_file(
            &static_root
                .canonicalize()
                .expect("failed to canonicalize assets dir"),
            "/assets",
            "/assets/app.css",
            &[String::from("index.html")],
        )
        .expect("path should be valid")
        .expect("asset should resolve");

        assert_eq!(
            resolved.file_name().and_then(|value| value.to_str()),
            Some("app.css")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn resolves_directory_request_using_custom_index_candidates() {
        let root = create_test_site();
        let docs_root = root.join("docs-site");
        fs::create_dir_all(docs_root.join("guide")).expect("failed to create docs dir");
        fs::write(docs_root.join("guide").join("home.html"), "<h1>guide</h1>")
            .expect("failed to write custom index");

        let resolved = resolve_static_file(
            &docs_root
                .canonicalize()
                .expect("failed to canonicalize docs root"),
            "/docs",
            "/docs/guide/",
            &[String::from("home.html"), String::from("index.html")],
        )
        .expect("path should be valid")
        .expect("custom index should resolve");

        assert_eq!(
            resolved.file_name().and_then(|value| value.to_str()),
            Some("home.html")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn rejects_path_traversal_attempt() {
        let root = create_test_site();
        let result = resolve_static_file(&root, "/", "/../secret.txt", &[String::from("index.html")]);

        assert!(matches!(result, Err(PathResolutionError::TraversalAttempt)));

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn preserves_legacy_extensionless_html_resolution() {
        let root = create_test_site();
        fs::write(root.join("about.html"), "<h1>about</h1>").expect("failed to write file");

        let resolved = resolve_static_file(&root, "/", "/about", &[String::from("index.html")])
            .expect("path should be valid")
            .expect("file should resolve");

        assert_eq!(
            resolved.file_name().and_then(|value| value.to_str()),
            Some("about.html")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn parse_request_head_extracts_path_and_query() {
        let request = parse_request_head(
            b"GET /docs/index.html?lang=zh HTTP/1.1\r\nHost: example.test\r\n\r\n",
        )
        .expect("request should parse");

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/docs/index.html");
        assert_eq!(request.query, Some(String::from("lang=zh")));
        assert_eq!(request.host(), Some("example.test"));
    }

    #[test]
    fn normalizes_host_without_port() {
        assert_eq!(
            normalize_host(Some("Example.TEST:8080")),
            Some(String::from("example.test"))
        );
    }

    #[test]
    fn group_servers_by_listener_keeps_shared_bindings() {
        let config = LoomisConfig {
            servers: vec![
                build_server(
                    "127.0.0.1:3000",
                    vec![String::from("example.test")],
                    vec![build_static_location("/")],
                ),
                build_server(
                    "127.0.0.1:3000",
                    vec![String::from("api.test")],
                    vec![LocationConfig {
                        path: String::from("/api"),
                        handler: LocationHandler::Proxy {
                            upstream: ProxyTarget {
                                host: String::from("127.0.0.1"),
                                port: 4000,
                                base_path: String::from("/"),
                            },
                        },
                        index: vec![String::from("index.html")],
                    }],
                ),
            ],
        };

        let listeners = group_servers_by_listener(&config);

        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].servers.len(), 2);
    }

    fn build_server(listen: &str, server_names: Vec<String>, locations: Vec<LocationConfig>) -> ServerConfig {
        ServerConfig {
            listen: listen.parse::<SocketAddr>().expect("listen should parse"),
            server_names,
            locations,
        }
    }

    fn build_static_location(path: &str) -> LocationConfig {
        let root = create_test_site();
        LocationConfig {
            path: path.to_string(),
            handler: LocationHandler::Static { root },
            index: vec![String::from("index.html")],
        }
    }

    fn create_test_site() -> PathBuf {
        let unique = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("loomis-server-test-{unique}"));
        fs::create_dir_all(&root).expect("failed to create temp dir");
        root.canonicalize()
            .expect("failed to canonicalize temp dir")
    }
}
