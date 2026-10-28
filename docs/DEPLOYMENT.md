# Docker 部署

当前部署使用 Linux x86_64 发布二进制与前端构建产物制作镜像，词典数据通过独立目录持久化。

## 构建镜像

在 Linux x86_64 构建机中运行（Node.js 22.18+、Rust 1.87+、C/C++ 工具链及 OpenSSL 开发包）：

```sh
npm ci
npm run build
cargo build --release --locked -p fastdict
docker build -t fastdict:latest .
docker run --rm fastdict:latest --version
docker save fastdict:latest | gzip > /tmp/fastdict-image.tar.gz
```

Docker 构建上下文仅包含发布二进制与 `dist`，不包含词典、数据库、SSH 文件和环境变量。运行镜像使用 Debian trixie；发布二进制的运行库必须与该镜像兼容，镜像构建后应执行上面的版本检查。

镜像包含 `speex` 包，用于将 Duden 等词典的 `.spx` 发音按需转换成 WAV。直接运行服务或执行音频测试的主机也需要安装 `speexdec`（Debian/Ubuntu：`apt install speex`）。转换使用临时文件，原 MDD 保持不变；进程内缓存上限为 8 MiB，单次转换限时 5 秒。

## 启动

将镜像包和仓库根目录的 `compose.yaml` 复制到服务器，在 `/opt/fastdict` 放置 Compose 文件。用 `docker load -i` 导入镜像。

在 `/opt/fastdict/.env` 配置以下三个变量，并将文件权限设为 `600`：

- `FASTDICT_IMAGE_TAG`：已导入的镜像标签。
- `FASTDICT_TOKEN`：使用 `openssl rand -hex 32` 生成的独立访问令牌。
- `FASTDICT_PUBLIC_ORIGIN`：你的访问地址，例如 `https://dict.example.com`。

```sh
cd /opt/fastdict
install -d -m 750 -o 10001 -g 10001 data
docker compose up -d
docker compose ps
```

容器以非 root 用户运行，应用只通过宿主机 `127.0.0.1:8765` 暴露给反向代理，数据位于 `/opt/fastdict/data`。Compose 自动重启容器，并检查需要认证的健康接口。

## HTTPS

在现有 Caddy 配置中添加独立站点：

```caddyfile
dict.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:8765 {
        lb_try_duration 20s
        lb_try_interval 250ms
    }
}
```

域名应解析到部署服务器。先验证完整 Caddy 配置，再按当前 Caddy 的重载方式生效。浏览器在“设置 → 连接与安全”中填写访问令牌；同源部署无需填写服务地址。

代理为容器切换时的短暂断连保留 20 秒重试窗口，避免状态刷新立即出现 502。沿用 Caddy 的默认重试条件，不自动重放已发送的上传或修改请求。参见 [Caddy 重试说明](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#load-balancing)。

## 更新与恢复

每次发布使用新的镜像标签，导入镜像后更新 `.env` 的 `FASTDICT_IMAGE_TAG`，执行 `docker compose up -d`。回滚时改回上一标签并重新执行该命令。数据目录保留在镜像之外；升级前从应用导出包含词典文件的备份。

查看运行日志：`docker compose logs --tail=100 fastdict`。不要提交 `.env`、镜像包或数据目录。
