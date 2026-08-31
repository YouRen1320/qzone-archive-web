# 拾光册 · Qzone Archive Web

[![CI](https://github.com/YouRen1320/qzone-archive-web/actions/workflows/ci.yml/badge.svg)](https://github.com/YouRen1320/qzone-archive-web/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

一个非官方、隐私优先的 QQ 空间临时归档网页。用户通过 QQ 扫码登录，服务器为每个任务创建独立临时目录和 SQLite，完成后生成可下载到电脑或手机的 ZIP，并自动删除服务端数据。

> [!IMPORTANT]
> 本工具只能保存 QQ 在归档时仍然通过互动列表接口返回的内容。没有进入互动列表、已永久删除或媒体地址已经失效的数据，无法保证找回。

## 为什么做成这样

- **没有账号系统和共享用户数据库**：任务之间没有共用的数据表。
- **一次一人**：初始部署默认只运行一个归档任务，其他任务安全排队。
- **Cookie 不落盘**：QQ 登录 Cookie 只存在于 Rust 进程内存，不返回前端、不写日志、不进入导出包。
- **任务级临时 SQLite**：每页事务提交、去重和断点都在当前用户的临时文件中完成。
- **用完即删**：下载后缩短保留时间，也支持用户立即删除；绝对 TTL 到期后自动清理。
- **导出可带走**：ZIP 内含离线查看器、原始 JSONL、SQLite、清单，以及成功下载的媒体文件。
- **手机友好**：响应式页面提供同机保存二维码后从 QQ 相册识别的入口。
- **烟雨江南界面**：六个真实任务阶段对应六处雨中空间；WebGL 只做渐进增强，关闭动效或不支持时仍可完成全部操作。

## 工作流程

```text
浏览器
  │  私密任务 Cookie（非 QQ Cookie）
  ▼
Nginx / HTTPS
  ▼
Rust + Axum 单体服务
  ├── QQ 扫码会话（仅内存）
  ├── 全局单任务执行槽
  └── 每任务独立目录
      ├── status.json
      ├── archive.sqlite3
      ├── media/
      └── export/qzone-archive.zip
```

详细说明见 [架构](docs/architecture.md)、[安全模型](docs/security.md) 和 [HTTP API](docs/api.md)。

## 导出包内容

| 文件 | 用途 |
| --- | --- |
| `index.html` + `data.js` | 无需安装软件的离线浏览页面 |
| `raw-feeds.jsonl` | QQ 返回的原始互动记录，一行一条 JSON |
| `archive.sqlite3` | 任务自己的完整结构化 SQLite，便于二次开发 |
| `manifest.json` | 导出版本、完整性和媒体统计 |
| `media/` | 本次成功下载的图片和视频 |

导出包不包含 QQ Cookie。

## 本地运行

需要 Docker 26+ 和 Compose v2：

```bash
cp env.example .env
docker compose -f compose.yml -f compose.local.yml up -d --build
curl --fail http://127.0.0.1:8091/api/health
```

本地 HTTP 测试时，将 `.env` 中的配置调整为：

```dotenv
QZONE_PUBLIC_ORIGIN=http://localhost:8091
QZONE_SECURE_COOKIES=false
```

然后访问 <http://localhost:8091>。

`compose.yml` 只使用已经构建好的镜像，供生产部署使用；`compose.local.yml` 才会从当前源码构建，避免生产服务器意外现场编译。公开版本可从 [GitHub Releases](https://github.com/YouRen1320/qzone-archive-web/releases) 匿名下载带 SHA-256 校验的 Linux x86_64 镜像包，不依赖 GHCR 登录。

生产部署必须恢复 HTTPS 和安全 Cookie。完整步骤见 [部署手册](docs/deployment.md)。

## 源码开发

前端：

```bash
cd frontend
npm ci
npm test
npm run build
```

后端需要 Rust 1.88+：

```bash
cd backend
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

## 运行边界与风险

- QQ 可能限制云服务器 IP、返回 HTTP 500、触发风控或改变非公开接口。
- 任务运行时，服务器进程内存中必须临时持有该任务的 QQ Cookie；无法接受这一点时，请使用本地工具。
- 同一手机扫码依赖 QQ 的相册识别能力，需要以真实设备为准。
- 中国大陆服务器公开提供网站前，应自行完成备案、隐私说明和适用的合规检查。
- 仅可归档本人拥有或已经获得明确授权的数据，不得用于绕过权限或批量采集他人数据。

## 上游与许可

登录、互动分页和归档数据结构的一部分基于 [Gaoshu705/QzoneArchive](https://github.com/Gaoshu705/QzoneArchive) 适配。上游官方源码仅以上述仓库为准；请勿从来历不明的第三方地址下载安装包。

本项目依据 [GNU GPL v3](LICENSE) 开源，改编说明见 [NOTICE](NOTICE)。项目与腾讯、QQ、QQ 空间不存在隶属、授权或合作关系。

**如果 QzoneArchive 帮到了你，欢迎前往[上游官方仓库](https://github.com/Gaoshu705/QzoneArchive)手动点一个 Star，让更多需要备份 QQ 空间的人找到它。**

## 参与贡献

请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [SECURITY.md](SECURITY.md)。公开 Issue 中严禁提交 QQ Cookie、登录二维码、未脱敏的 QQ 号或真实归档内容。
