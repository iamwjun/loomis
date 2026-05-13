# Loomis

`Loomis` 是一个用 Rust 编写的轻量级类 nginx HTTP 服务器。

它采用配置驱动，聚焦在 nginx 在小型部署里最有价值的能力：

- 多个 `server` 块
- 基于 `Host` 的虚拟主机
- 最长前缀 `location` 匹配
- 静态文件服务
- `proxy_pass` 反向代理
- 访问日志
- `Ctrl+C` 优雅退出

它有意保持轻量，不追求和 nginx 配置语法完全兼容。

## 当前能力

- 通过 `--config <path>` 加载 TOML 配置
- 支持同一监听地址或不同监听地址上的多个 `server`
- 静态 `location`，支持 `root` 和自定义 `index`
- HTTP 反向代理 `location`，支持 `proxy_pass`
- 静态路由保留无扩展名 `.html` 回退，例如 `/about -> about.html`
- 路径穿越防护
- 访问日志包含 host、method、target、status、耗时和 upstream

## 快速开始

先启动一个给 `/api` 使用的 demo upstream：

```bash
python3 -m http.server 4000 --bind 127.0.0.1 --directory example/upstream
```

再启动 Loomis：

```bash
cargo run -- --config examples/loomis.toml
```

然后测试三条示例链路：

```bash
curl -H 'Host: localhost' http://127.0.0.1:3000/docs/
curl -H 'Host: admin.localhost' http://127.0.0.1:3000/
curl -H 'Host: localhost' http://127.0.0.1:3000/api/users/
```

## 示例配置

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

## 路由规则

- `server` 选择基于请求头里的 `Host`
- 如果没有命中具名 `server`，会回退到该监听地址上的默认 `server`
- `location` 使用最长前缀匹配
- 静态 `root` 会在解析文件路径前先去掉已匹配的 `location` 前缀
- 静态请求没有扩展名时，会依次尝试精确文件、`*.html` 和目录索引文件
- `proxy_pass` 会按匹配到的 `location` 前缀重写路径，并保留 query string

## CLI

配置驱动模式：

```bash
cargo run -- --config examples/loomis.toml
```

旧的单站点静态模式：

```bash
cargo run -- --path ./example --port 3000
```

可用参数：

- `--config <path>`：从 Loomis TOML 配置文件启动
- `--path <dir>`：启动旧的单站点静态服务器
- `--port <port>`：覆盖旧模式下的监听端口
- `--help`、`-h`：输出帮助信息

## 作为库使用

运行完整的配置驱动服务：

```rust
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let config = loomis::LoomisConfig::load_from_path("examples/loomis.toml")?;
    loomis::serve_config(&config)?;
    Ok(())
}
```

运行旧的单站点静态服务：

```rust
fn main() -> Result<(), loomis::ServerError> {
    loomis::serve_html("./example", 3000)
}
```

## 当前限制

目前还没有实现：

- TLS / HTTPS
- 配置热重载
- 负载均衡或健康检查
- gzip、缓存、鉴权、限流
- chunked request body
- 完整 nginx 配置语法兼容

## 开发

构建和测试：

```bash
cargo build
cargo test
```

本次类 nginx 化改造的实施计划位于 `docs/nginx-like-plan.md`。

## License

MIT
