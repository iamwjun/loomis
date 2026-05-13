# Loomis

`Loomis` is a foundational library for web server applications built in Rust. Its goal is to provide server-side projects with a lightweight, composable, and maintainable set of common abstractions.

Rather than being an all-in-one framework, `Loomis` is positioned as an infrastructure-oriented library. It is meant to consolidate the common startup, organization, extension, and operational concerns of web services, so application code can stay focused on APIs and domain logic.

## Project Positioning

In a typical web service, beyond business logic itself, teams usually end up solving the same set of problems repeatedly:

- Application startup and lifecycle management
- Configuration loading and environment separation
- Routing, request handling, and response abstraction
- Error modeling and standardized error responses
- Logging, tracing, and observability
- Middleware support
- Graceful shutdown and runtime resource management
- Testing support and local development ergonomics

`Loomis` aims to extract those foundational capabilities that every project needs, but no team wants to keep reimplementing, into a reusable Rust library that can serve as the base layer for web server applications.

## Design Goals

- Type safety: use Rust's type system to enforce error boundaries and interface contracts.
- Modularity: keep capabilities decoupled and composable instead of binding everything to one execution model.
- Extensibility: make it easy to integrate middleware, centralized error handling, logging, and monitoring.
- Production-oriented: account for configuration, observability, stability, and lifecycle management from the start.
- Deliberate scope: focus on shared foundational capabilities without intruding on business-specific models.

## Use Cases

`Loomis` is suitable as a foundational library for:

- Internal API services
- Backend services for admin systems
- Microservices or lightweight service nodes
- Rust web applications that need a unified service foundation
- Teams building their own server-side infrastructure in Rust

## Current Status

The repository is still in an early initialization stage. The implementation is intentionally minimal, and the core API and module boundaries have not been fully developed yet.

That means:

- The overall direction of the project is already defined
- The repository has been set up as a library crate
- The foundational pieces for a web server base library will be added incrementally
- At this stage, it is better suited as a structural starting point than as a production-ready solution

If you plan to continue building on this repository, these module boundaries are good candidates to define first:

- `app`: application entrypoint, boot flow, and lifecycle management
- `config`: configuration loading, environment variables, and validation
- `http`: request/response abstractions and common response structures
- `error`: error types, conversions, and standardized error responses
- `middleware`: cross-cutting concerns such as logging, auth, CORS, and rate limiting
- `observability`: logging, tracing, and metrics
- `server`: listeners, graceful shutdown, and runtime parameter management

## Usage

The current implementation provides a minimal HTML server that serves `.html` and `.htm` files from a local directory.

### Run the bundled example

```bash
cargo run
```

By default, Loomis serves files from `./example` on `http://127.0.0.1:3000/`.

### Use a custom directory or port

```bash
cargo run -- --path ./example --port 8080
```

Available CLI options:

- `--path <html-dir>`: root directory to serve. The directory must exist.
- `--port <port>`: TCP port to bind to on `127.0.0.1`.
- `--help`, `-h`: print the command usage.

### Routing behavior

Loomis currently supports `GET` requests only and only serves HTML files. Request paths are resolved using these rules:

- `/` resolves to `index.html`
- `/about` resolves to `about.html` or `about/index.html`
- `/docs/` resolves to `docs/index.html`
- path traversal such as `/../secret.html` is rejected

### Use as a library

```rust
fn main() -> Result<(), loomis::ServerError> {
    loomis::serve_html("./example", 3000)
}
```

`serve_html` validates the root directory, binds to `127.0.0.1:<port>`, and serves matching HTML files until the process exits.

## Development

After cloning the repository, you can build and test it like a standard Rust library:

```bash
cargo build
cargo test
```

If the project evolves into a more complete server-side foundation library, these areas are worth prioritizing:

1. Crate-level module structure
2. A shared error model
3. Configuration and application startup flow
4. HTTP abstractions and middleware support
5. Logging and tracing integration
6. Example applications and integration tests

## Project Structure

The current repository structure is still quite simple:

```text
.
├── Cargo.toml
├── README.md
├── example
│   └── index.html
└── src
    ├── lib.rs
    └── main.rs
```

As more foundational capabilities are added, the directory layout should evolve into a clearer module structure to support long-term maintenance and extension.

## Vision

`Loomis` is intended to become a practical starting point for Rust web server applications:

- More efficient than rebuilding service infrastructure from scratch
- More controllable than adopting a heavyweight framework
- More consistent than scattering utility code across projects

If your goal is to establish a maintainable long-term foundation for Rust server-side development, this repository is a suitable place to start.

## License

MIT
