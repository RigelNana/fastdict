# fastdict

本地 MDX / MDD 词典应用。React 前端、Rust 服务和命令行共用一套 HTTP API。

- 词头联想、分组查词、历史记录与全文检索。
- 词典及 CSS、MDD、字体、图片、音频资源管理。
- 设置即时保存，正文独立沙箱，Dark Reader 动态深色适配。
- 索引管理、备份恢复，以及连接远端词典服务。

## 运行

需要 Node.js 22.18+（或更新的 LTS）、Rust 1.87+ 和 C/C++ 编译工具链。

```sh
git clone https://github.com/RigelNana/fastdict.git
cd fastdict
npm ci
npm run build
cargo run --locked -p fastdict -- serve
```

打开 <http://127.0.0.1:8765>，在设置中导入自己的词典。仓库不包含个人词典数据。

开发时保持服务运行，另一个终端执行 `npm run dev`，访问 <http://127.0.0.1:5173>。

## 命令行与数据

```sh
cargo run --locked -p fastdict -- --help
cargo run --locked -p fastdict -- suggest hello
cargo run --locked -p fastdict -- lookup hello
```

默认数据目录为 `.fastdict/`，可用 `serve --data-dir <目录>` 指定。已有 `.mydict/` 安装会继续读取原目录及 `mydict.sqlite3`，保留词典、设置、历史和资源路径。浏览器会迁移旧的会话设置。

远端客户端使用 `FASTDICT_ENDPOINT`、`FASTDICT_TOKEN` 或 `--token-file`。对外监听时必须配置至少 16 字符的令牌；反向代理使用 `FASTDICT_PUBLIC_ORIGIN`，额外允许的管理界面来源使用 `FASTDICT_ALLOWED_ORIGINS`。详见 [架构说明](docs/ARCHITECTURE.md) 和 [API 文档](docs/API.md)。

## 检查

```sh
npm run build
node --test tests/*.test.mjs
cargo test --locked -p fastdict
```

内置解析库位于 `vendor/wodix`，随仓库提供源代码，无需初始化子模块；来源和许可证见 [依赖说明](vendor/README.md)。
