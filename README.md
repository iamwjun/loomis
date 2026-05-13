# Loomis

`Loomis` is a lightweight nginx-like HTTP server written in Rust.

It is configuration-driven and focuses on the parts that make nginx useful in small deployments:

- multiple `server` blocks
- `Host`-based virtual hosts
- longest-prefix `location` matching
- static file serving
- `proxy_pass` reverse proxying
- access logs
- `Ctrl+C` graceful shutdown

It is intentionally smaller than nginx and does not try to be config-compatible with it.

## Features

- TOML config file loaded with `--config <path>`
- Multiple `server` blocks on the same or different listen addresses
- Static `location` blocks with `root` and configurable `index`
- HTTP reverse proxy `location` blocks with `proxy_pass`
- Extensionless `.html` fallback for static routes such as `/about -> about.html`
- Path traversal protection
- Access log lines including host, method, target, status, duration, and upstream

## Quick Start

Start a demo upstream server for `/api`:

```bash
python3 -m http.server 4000 --bind 127.0.0.1 --directory example/upstream
```

Start Loomis with the bundled config:

```bash
cargo run -- --config examples/loomis.toml
```

Then try the sample routes:

```bash
curl -H 'Host: localhost' http://127.0.0.1:3000/docs/
curl -H 'Host: admin.localhost' http://127.0.0.1:3000/
curl -H 'Host: localhost' http://127.0.0.1:3000/api/users/
```

## Example Config

```toml
[[server]]
listen = "127.0.0.1:3000"
server_name = ["localhost", "site.localhost"]

  [[server.location]]
  path = "/"
  root = "../example"

  [[server.location]]
  path = "/docs"
  root = "../example/docs"

  [[server.location]]
  path = "/api"
  proxy_pass = "http://127.0.0.1:4000"

[[server]]
listen = "127.0.0.1:3000"
server_name = ["admin.localhost"]

  [[server.location]]
  path = "/"
  root = "../example/admin"
```

## Routing Rules

- `server` selection is based on the `Host` header.
- If no named `server` matches, Loomis falls back to the default server on that listener.
- `location` selection uses longest prefix matching.
- Static `root` locations strip the matched location prefix before resolving the filesystem path.
- If a static request has no extension, Loomis tries the exact file, then `*.html`, then directory index candidates.
- `proxy_pass` rewrites the request path relative to the matched location prefix and preserves the query string.

## CLI

Config-driven mode:

```bash
cargo run -- --config examples/loomis.toml
```

Legacy single-site static mode:

```bash
cargo run -- --path ./example --port 3000
```

Available options:

- `--config <path>`: start from a Loomis TOML config file
- `--path <dir>`: start the legacy single-site static server
- `--port <port>`: override the legacy static server port
- `--help`, `-h`: print usage

## Library Usage

Run the full config-driven server:

```rust
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let config = loomis::LoomisConfig::load_from_path("examples/loomis.toml")?;
    loomis::serve_config(&config)?;
    Ok(())
}
```

Run the legacy single-site static server:

```rust
fn main() -> Result<(), loomis::ServerError> {
    loomis::serve_html("./example", 3000)
}
```

## Limitations

Loomis currently does not implement:

- TLS / HTTPS
- config hot reload
- load balancing or health checks
- gzip, caching, auth, or rate limiting
- chunked request bodies
- full nginx configuration syntax compatibility

## Development

Build and test:

```bash
cargo build
cargo test
```

The implementation plan for the nginx-like expansion lives at `docs/nginx-like-plan.md`.

## License

MIT
