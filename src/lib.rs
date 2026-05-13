use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

#[derive(Debug)]
pub enum ServerError {
    InvalidRootDirectory(PathBuf),
    Io(io::Error),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRootDirectory(path) => {
                write!(f, "invalid html root directory: {}", path.display())
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<io::Error> for ServerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn serve_html(root_dir: impl AsRef<Path>, port: u16) -> Result<(), ServerError> {
    let root_dir = canonicalize_root(root_dir.as_ref())?;
    let address = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&address)?;

    println!(
        "Serving HTML from {} at http://{address}/",
        root_dir.display()
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root_dir = root_dir.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &root_dir) {
                        eprintln!("connection error: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept error: {error}"),
        }
    }

    Ok(())
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

fn handle_connection(mut stream: TcpStream, root_dir: &Path) -> io::Result<()> {
    let mut buffer = [0_u8; 8 * 1024];
    let read_len = stream.read(&mut buffer)?;

    if read_len == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..read_len]);
    let Some((method, request_path)) = parse_request_line(&request) else {
        return write_text_response(&mut stream, "400 Bad Request", "Malformed HTTP request.");
    };

    if method != "GET" {
        return write_text_response(
            &mut stream,
            "405 Method Not Allowed",
            "Only GET requests are supported.",
        );
    }

    match resolve_request_file(root_dir, request_path) {
        Ok(Some(file_path)) => match fs::read(&file_path) {
            Ok(body) => write_response(&mut stream, "200 OK", "text/html; charset=utf-8", &body),
            Err(_) => write_text_response(
                &mut stream,
                "500 Internal Server Error",
                "Failed to read the requested HTML file.",
            ),
        },
        Ok(None) => write_text_response(&mut stream, "404 Not Found", "HTML file not found."),
        Err(PathResolutionError::TraversalAttempt) => {
            write_text_response(&mut stream, "400 Bad Request", "Invalid request path.")
        }
    }
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;
    Some((method, path))
}

fn resolve_request_file(
    root_dir: &Path,
    request_path: &str,
) -> Result<Option<PathBuf>, PathResolutionError> {
    let request_target = request_path
        .split('?')
        .next()
        .unwrap_or(request_path)
        .split('#')
        .next()
        .unwrap_or(request_path);
    let sanitized_segments = sanitize_segments(request_target)?;
    let mut base_path = root_dir.to_path_buf();

    for segment in &sanitized_segments {
        base_path.push(segment);
    }

    let mut candidates = Vec::new();
    if sanitized_segments.is_empty() || request_target.ends_with('/') {
        candidates.push(base_path.join("index.html"));
    } else if base_path.extension().is_none() {
        candidates.push(base_path.with_extension("html"));
        candidates.push(base_path.join("index.html"));
    } else if is_html_file(&base_path) {
        candidates.push(base_path);
    } else {
        return Ok(None);
    }

    for candidate in candidates {
        if !candidate.is_file() || !is_html_file(&candidate) {
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

fn is_html_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some(ext) if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm")
    )
}

fn write_text_response(stream: &mut TcpStream, status: &str, message: &str) -> io::Result<()> {
    write_response(
        stream,
        status,
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[derive(Debug)]
enum PathResolutionError {
    TraversalAttempt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_index_file_for_root_request() {
        let root = create_test_html_tree();
        let resolved = resolve_request_file(&root, "/")
            .expect("path should be valid")
            .expect("index.html should exist");

        assert_eq!(
            resolved.file_name().and_then(OsStr::to_str),
            Some("index.html")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn resolves_extensionless_request_to_html_file() {
        let root = create_test_html_tree();
        let resolved = resolve_request_file(&root, "/about")
            .expect("path should be valid")
            .expect("about.html should exist");

        assert_eq!(
            resolved.file_name().and_then(OsStr::to_str),
            Some("about.html")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn resolves_directory_request_to_nested_index_file() {
        let root = create_test_html_tree();
        let resolved = resolve_request_file(&root, "/docs/")
            .expect("path should be valid")
            .expect("docs/index.html should exist");

        assert_eq!(
            resolved
                .parent()
                .and_then(Path::file_name)
                .and_then(OsStr::to_str),
            Some("docs")
        );
        assert_eq!(
            resolved.file_name().and_then(OsStr::to_str),
            Some("index.html")
        );

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    #[test]
    fn rejects_path_traversal_attempt() {
        let root = create_test_html_tree();
        let result = resolve_request_file(&root, "/../secret.html");

        assert!(matches!(result, Err(PathResolutionError::TraversalAttempt)));

        fs::remove_dir_all(root).expect("failed to clean up test directory");
    }

    fn create_test_html_tree() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("loomis-test-{unique}"));

        fs::create_dir_all(root.join("docs")).expect("failed to create test directories");
        fs::write(root.join("index.html"), "<h1>home</h1>").expect("failed to write index.html");
        fs::write(root.join("about.html"), "<h1>about</h1>").expect("failed to write about.html");
        fs::write(root.join("docs").join("index.html"), "<h1>docs</h1>")
            .expect("failed to write docs/index.html");

        root.canonicalize()
            .expect("failed to canonicalize test directory")
    }
}
