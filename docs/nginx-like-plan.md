# Loomis 类 Nginx 化实施计划

## 背景

当前 `loomis` 仅支持：

- 单目录 HTML 静态文件服务
- 固定监听 `127.0.0.1:<port>`
- 极简 CLI 参数解析

这和“类 nginx 应用”之间还缺少配置驱动、虚拟主机、`location` 路由、反向代理、日志与生命周期管理等核心能力。

## 目标

本轮将 `loomis` 扩展成一个可通过配置文件启动的轻量反向代理与静态资源服务器，交付一套明确的、可测试的 Nginx 风格能力：

1. 使用配置文件定义一个或多个 `server`
2. 每个 `server` 支持 `listen`、`server_name`、`location`
3. `location` 支持两种动作：
   - `root` / `index`：静态文件服务
   - `proxy_pass`：反向代理到上游 HTTP 服务
4. 按最长前缀匹配选择 `location`
5. 输出基础访问日志
6. 支持优雅关闭

## 非目标

本轮不实现以下能力：

- TLS / HTTPS
- 热重载配置
- 负载均衡与健康检查
- gzip、缓存、限流、鉴权
- 完整的 Nginx 配置语法兼容

## 实施阶段

### 阶段 1：配置驱动入口

交付内容：

- 新增 `loomis.toml` 配置模型
- CLI 增加 `--config <path>`，从配置文件启动
- 抽离 `app` / `config` / `server` 基础模块
- 支持多 `server` 配置解析与校验

验收标准：

- 可从 TOML 文件加载配置
- 缺失字段、非法端口、重复监听地址等错误可读

提交策略：

- 完成后提交 1 个 commit

### 阶段 2：静态路由与虚拟主机

交付内容：

- 基于 `Host` 头匹配 `server`
- 基于最长前缀匹配 `location`
- `root` / `index` 静态文件服务
- 保留目录穿越防护

验收标准：

- `/`、`/docs/`、`/about` 等路径按配置解析
- 不同 `server_name` 可命中不同站点
- 非法路径返回 400

提交策略：

- 完成后提交 1 个 commit

### 阶段 3：反向代理与运行时能力

交付内容：

- `proxy_pass` 前缀代理
- 转发请求头与响应体
- 访问日志
- `Ctrl+C` 触发优雅关闭

验收标准：

- `/api/*` 可转发到上游 HTTP 服务
- 访问日志记录方法、路径、状态码、耗时
- 收到终止信号后停止接受新连接并退出

提交策略：

- 完成后提交 1 个 commit

### 阶段 4：文档与验证

交付内容：

- 更新 `README.md` 与 `README.zh.md`
- 补充配置示例与运行说明
- 增加单元测试与必要的集成测试

验收标准：

- `cargo test` 通过
- 文档能覆盖核心配置与运行方式

提交策略：

- 若文档和验证仅收尾于前一阶段，可并入最后一个功能 commit；否则单独提交 1 个 commit

## 预期目录演进

```text
.
├── docs/
│   └── nginx-like-plan.md
├── example/
│   └── index.html
├── examples/
│   └── loomis.toml
└── src/
    ├── app.rs
    ├── config.rs
    ├── http.rs
    ├── lib.rs
    ├── main.rs
    └── server.rs
```

## 完成定义

满足以下条件才视为完成：

- `docs/nginx-like-plan.md` 已存在且内容可执行
- 至少具备配置驱动、多 `server`、`location` 静态服务、`proxy_pass`、访问日志、优雅关闭
- 每个功能阶段至少对应一个独立 commit
- 测试通过，文档可指导运行
