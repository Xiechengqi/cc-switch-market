# cc-switch-market

`cc-switch-market` 是面向 `cc-switch` / `cc-switch-router` 的 token 交易市场服务。它负责用户门户、充值、API key、模型价格、请求路由、用量扣费、provider 收益、提现、工单和运营后台。

## 系统架构

整体请求链路：

```text
API 用户
  -> cc-switch-market
  -> cc-switch-router market proxy
  -> cc-switch client tunnel
  -> 上游模型服务
```

核心原则：

- market 不读取、不保存 provider 的 `share_token` / 上游 API key 明文。
- router 负责校验 market 身份、share 授权和 tunnel 转发。
- market 负责 API 用户余额、用量计费、provider 收益和提现结算。
- token 交易抽成由 Market 抽成 `MARKET_PLATFORM_COMMISSION_BPS` 与 Router 抽成 `MARKET_ROUTER_COMMISSION_BPS` 共同组成，默认 10% + 5%；provider 收入按消费金额扣除总抽成后的净额记账。
- 平台记录充值手续费、提现手续费和 Market token 交易抽成；Router 抽成进入 `router@[router_host]` 的 provider 余额。
- 所有资金变化都必须进入 ledger，可通过用户侧、provider 侧和 admin 侧查询。

主要模块：

- `market-api`：Axum HTTP API、错误响应、路由入口。
- `market-auth`：router Resend 邮箱验证码登录、market Web session、API key、admin 判定。
- `market-wallet`：账户、ledger、预授权、结算和人工调账。
- `market-pricing`：模型价格、价格快照、历史可复算。
- `market-proxy`：OpenAI / Anthropic 兼容 API 入口和用量记账。
- `market-settlement`：provider claim、Gate.io 自动提现、人工提现。
- `market-support`：反馈工单、提现工单、附件和 admin 处理流。
- `market-router-client`：同步 router shares、选路和 market proxy 调用。

## 技术框架

后端：

- Rust 2024
- Axum / Tower / Tokio
- libSQL / SQLite / Turso
- Reqwest
- Rust Decimal
- HMAC-SHA512 / SHA-256
- Rust Embed：将前端静态文件内嵌进 Rust binary

前端：

- Next.js App Router
- React 19
- TypeScript
- Tailwind CSS
- Lucide React
- Playful Geometric 视觉风格

外部服务：

- cc-switch-router Resend 邮箱验证码：Web 用户、provider、admin 登录身份源。
- 本地 SQLite / Turso：结构化业务数据与资金 ledger 的事实源。默认使用本地 `$HOME/.config/cc-switch-market/cc-switch-market.db`，配置 Turso 后使用远程 Turso + 本地 embedded replica。
- 对象存储：默认使用本地 `$HOME/.config/cc-switch-market/objects`，保存 webhook 原文、请求调试包、提现凭证、工单附件等不可变对象；Cloudflare R2 仅作为后续生产扩展预留。
- Dodo Payments：用户充值。
- Gate.io API V4：provider Gate.io 自动提现。
- cc-switch-router：market proxy、share 在线状态、share 授权。

当前实现说明：

- Dodo webhook 使用 Standard Webhooks 风格的 `webhook-id`、`webhook-timestamp`、`webhook-signature` 校验，并用 webhook id 做幂等。
- Gate.io 自动提现启用后调用 API V4 `POST /api/v4/withdrawals/push`，只支持纯数字 Gate.io 用户 UID（`receive_uid`），不支持邮箱或手机；请求/响应原文写入对象存储，未知或失败状态进入人工复核。
- OpenAI `/v1/chat/completions` 支持 `stream=true`，market 会强制注入 `stream_options.include_usage=true`，边转发 SSE 边解析 usage；无 usage、上游中断或客户端断开会进入 `needs_review`，由 admin 在 `/admin/charges` 手动结算或释放。
- Anthropic `/v1/messages` streaming 暂不开放，继续显式拒绝，避免在未完成 provider-specific usage tee 前出现资金风险。

## 资金模型

账户类型：

- `user_cash`：API 用户可用余额。
- `user_reserved`：请求预授权锁定余额。
- `client_payable`：provider 待提现余额。
- `payout_reserved`：provider 已发起提现、等待处理的锁定余额。
- `payment_clearing`：充值/退款清算账户。
- `settlement_paid`：已完成提现。
- `fee_revenue`：充值/提现手续费收入与 Market token 交易抽成。
- `risk_loss`：风控损失和人工补偿账户。

关键账务路径：

```text
用户消费:
user_cash -> client_payable
user_cash -> fee_revenue

provider 发起提现:
client_payable -> payout_reserved

提现成功:
payout_reserved -> settlement_paid
payout_reserved -> fee_revenue

提现失败 / 取消:
payout_reserved -> client_payable
```

admin 不能直接修改余额缓存。所有人工打款、补偿、扣减都必须创建 ledger transaction，并关联工单或审计记录。

## 工单系统

工单类型：

- `feedback`：使用反馈、bug、需求。
- `payout_manual`：provider 选择非 Gate.io 收款方式后的人工提现工单。
- `billing_issue`：充值、扣费、退款争议。
- `account_issue`：账号、API key、风控问题。

人工提现流程：

```text
provider 在 /claim 选择其他收款方式
  -> 创建 payout_request(method=manual)
  -> 锁定 client_payable 到 payout_reserved
  -> 自动创建 payout_manual 工单
  -> admin 手动打款
  -> admin 上传/填写凭证
  -> complete-manual-payout 完成 ledger 结算
```

## 二进制部署

`cc-switch-market` 的前端静态页面已经内嵌到 Rust binary。部署时只需要发布一个二进制文件：

```text
target/release/cc-switch-market
```

运行：

```bash
./cc-switch-market
```

首次运行会自动生成默认配置文件：

```text
$HOME/.config/cc-switch-market/.env
```

查看命令帮助、交互式配置和当前生效配置：

```bash
./cc-switch-market help
./cc-switch-market config
./cc-switch-market config show
./cc-switch-market config show --masked
./cc-switch-market config path
```

服务启动后：

- `/v1/*` 为后端 API。
- 其他路径由 binary 内嵌的前端静态文件响应。
- `/`、`/dashboard`、`/claim`、`/support`、`/admin/*` 都由同一个 binary 提供。

## 运行时配置

默认配置文件会生成到 `$HOME/.config/cc-switch-market/.env`。可以用 `./cc-switch-market config` 交互式修改，也可以手动编辑。

| 变量名 | 默认值 | 说明 |
| --- | --- | --- |
| `MARKET_HTTP_ADDR` | `0.0.0.0:8080` | market 本地 HTTP 监听地址。只影响进程绑定哪个 IP/端口，不代表公网访问地址。 |
| `MARKET_TUNNEL_ENABLED` | `true` | 是否在启动后自动向 router 申请 market tunnel lease，并把 `ROUTER_MARKET_SUBDOMAIN.ROUTER_BASE_DOMAIN` 转发到本地 `MARKET_HTTP_ADDR`。本地仅调试 HTTP 服务时可设为 `false`。 |
| `RUST_LOG` | `cc_switch_market=info,tower_http=info,axum=info` | Rust 日志过滤规则。生产环境可改为 `cc_switch_market=info,tower_http=warn,axum=warn`。 |
| `MARKET_SESSION_COOKIE_NAME` | `cc_switch_market_session` | market Web 登录使用的 HttpOnly session cookie 名称。 |
| `MARKET_SESSION_COOKIE_SECRET` | `change-me-market-session-secret-32b` | market Web session token hash pepper。生产环境必须改成高强度随机值。 |
| `MARKET_SESSION_TTL_SECS` | `2592000` | market Web session 有效期，默认 30 天。 |
| `MARKET_ADMIN_EMAILS` | `admin@example.com` | market 管理员邮箱白名单，多个邮箱用英文逗号分隔。后端根据已验证 session email 判定 admin。 |
| `MARKET_MIN_REQUEST_BALANCE` | `0.10` | API 请求进入 router 前要求用户至少具备的余额门槛；实际扣费仍按 market 解析到的 usage 和价格快照结算。 |
| `MARKET_PLATFORM_COMMISSION_BPS` | `1000` | token 用量消费的 Market 抽成比例，单位为 basis points；默认 1000 即 10%。 |
| `MARKET_ROUTER_COMMISSION_BPS` | `500` | token 用量消费的 Router 抽成比例，单位为 basis points；默认 500 即 5%。Market + Router 总抽成不能超过 10000。Router 抽成进入 `router@[router_host]` 的 provider 余额。 |
| `MARKET_SQLITE_PATH` | `$HOME/.config/cc-switch-market/cc-switch-market.db` | 本地 SQLite 数据库路径。仅在未配置 `TURSO_DATABASE_URL` 时生效。 |
| `TURSO_DATABASE_URL` | 空 | Turso 远程数据库 URL，必须以 `libsql://` 开头。为空时使用本地 SQLite。只要配置了 Turso，连接失败就直接启动失败，不会回退到本地 SQLite。 |
| `TURSO_AUTH_TOKEN` | 空 | Turso 认证 token。配置 `TURSO_DATABASE_URL` 时必填。 |
| `TURSO_REPLICA_PATH` | `$HOME/.config/cc-switch-market/turso-replica.db` | Turso embedded replica 本地路径。 |
| `TURSO_SYNC_INTERVAL_SECS` | `300` | Turso replica 同步间隔预留配置。 |
| `TURSO_BACKUP_ENABLED` | `true` | 使用 Turso 时是否定时把本地 replica 复制到备份目录。 |
| `TURSO_BACKUP_INTERVAL_SECS` | `3600` | Turso 本地备份间隔，默认每小时。 |
| `TURSO_BACKUP_RETENTION_DAYS` | `7` | Turso 本地备份保留天数，默认只保留最近 1 周。 |
| `OBJECT_STORE_BACKEND` | `local` | 对象存储后端。当前 binary 支持 `local`；`r2` 为生产扩展预留，配置后会 fail-fast。 |
| `OBJECT_STORE_LOCAL_DIR` | `$HOME/.config/cc-switch-market/objects` | 本地对象存储目录。 |
| `REQUEST_OBJECT_RETENTION_DAYS` | `7` | API 请求调试对象保留天数。仅自动清理已终态且没有未关闭工单的 request body / response meta 对象，sha256 和账务记录会保留。 |
| `REQUEST_OBJECT_CLEANUP_BATCH_SIZE` | `1000` | 每轮维护任务最多清理的请求调试对象记录数，用于限制单次清理压力。 |
| `R2_ACCOUNT_ID` | 空 | Cloudflare R2 account id，当前预留未启用。 |
| `R2_ACCESS_KEY_ID` | 空 | Cloudflare R2 access key id，当前预留未启用。 |
| `R2_SECRET_ACCESS_KEY` | 空 | Cloudflare R2 secret access key，当前预留未启用。 |
| `R2_BUCKET` | 空 | Cloudflare R2 bucket，当前预留未启用。 |
| `R2_PUBLIC_BASE_URL` | 空 | Cloudflare R2 公网访问基础地址，当前预留未启用。 |
| `ROUTER_BASE_DOMAIN` | `localhost:8081` | router 基础域名，只填域名或 `host:port`，不要带 `http://` / `https://`。market 会推导 router API 地址为 `http(s)://{ROUTER_BASE_DOMAIN}`。 |
| `ROUTER_MARKET_SUBDOMAIN` | `market` | market 在 router wildcard tunnel 下使用的子域名前缀。market 公网地址由它和 `ROUTER_BASE_DOMAIN` 推导。 |
| `MARKET_DISPLAY_NAME` | `Main Market` | router dashboard 和 market 列表中展示的 market 名称。market 身份邮箱不通过 env 配置，而是来自 `cc-switch-market login` 的 router 邮箱验证码登录。 |
| `DODO_API_BASE` | `https://test.dodopayments.com` | Dodo Payments API 地址。配置 `DODO_API_KEY` 和 `DODO_PRODUCT_ID` 后会创建真实 checkout session；否则保留本地 mock checkout。 |
| `DODO_API_KEY` | 空 | Dodo Payments API key。为空时不调用 Dodo，便于本地开发。 |
| `DODO_PRODUCT_ID` | 空 | Dodo top-up 产品 ID。建议配置为 0.01 USD 单价产品，market 用充值金额换算 quantity。 |
| `DODO_ALLOWED_PAYMENT_METHOD_TYPES` | `credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency` | 创建 Dodo checkout 时允许的支付方式。默认开启信用卡、借记卡、Apple Pay、Google Pay、WeChat Pay、Crypto & Stablecoins。这里必须使用 Dodo API 枚举值，例如微信是 `we_chat_pay`，加密货币是 `crypto_currency`。 |
| `DODO_WEBHOOK_SECRET` | `dev` | Dodo Payments webhook 校验密钥。生产环境必须使用 Dodo 后台提供的真实 secret。 |
| `DODO_MOCK_CHECKOUT_ENABLED` | `true` | Dodo 未配置时是否允许本地 mock checkout。生产环境建议设为 `false`，此时必须配置 `DODO_API_KEY` 和 `DODO_PRODUCT_ID`。 |
| `GATEIO_API_BASE` | `https://api.gateio.ws` | Gate.io API 基础地址。通常保持默认。 |
| `GATEIO_API_KEY` | 空 | Gate.io API key。仅启用自动提现时需要。 |
| `GATEIO_API_SECRET` | 空 | Gate.io API secret。仅启用自动提现时需要，必须妥善保护。 |
| `GATEIO_SETTLEMENT_CURRENCY` | `USDT` | provider 自动提现使用的 Gate.io 结算币种；自动提现目标必须填写纯数字 Gate.io 用户 UID。 |
| `GATEIO_USD_USDT_RATE` | `1.000000` | USD 到 USDT 的结算换算率。MVP 默认按 1:1。 |
| `GATEIO_AUTO_PAYOUT_ENABLED` | `false` | 是否启用 Gate.io 自动提现。为 `false` 时提现走人工工单/手动处理路径。 |
| `GATEIO_PAYOUT_WORKER_INTERVAL_SECS` | `60` | Gate.io 自动提现 worker 扫描 pending payout 的间隔。 |

数据库默认不需要额外配置，会自动创建本地 SQLite 文件。配置 `TURSO_DATABASE_URL` 后进入 Turso 模式；此时 `TURSO_AUTH_TOKEN` 必填，且远程连接失败会直接报错，避免误写到本地 SQLite。对象存储默认也不需要额外配置，会写入本地 `objects` 目录。

Turso 模式下会使用 embedded replica。备份文件默认写入：

```text
$HOME/.config/cc-switch-market/turso-db-backup/
```

`MARKET_PUBLIC_BASE_URL` 不需要手工配置，默认由 `ROUTER_BASE_DOMAIN` 和 `ROUTER_MARKET_SUBDOMAIN` 推导。例如 `ROUTER_BASE_DOMAIN=jptokenswitch.cc` 且 `ROUTER_MARKET_SUBDOMAIN=market-a` 时，market 公网地址为 `https://market-a.jptokenswitch.cc`，router API 地址为 `https://jptokenswitch.cc`。

首次启动前必须先执行 `./cc-switch-market login`，用邮箱验证码登录 router。登录成功后会保存 `$HOME/.config/cc-switch-market/router-session.json`，market 启动时用该 session 自动注册 / 续期 router market registry，并使用同一个 session 调用 `/v1/market/shares`、`/v1/markets/tunnel/lease` 和 market proxy。

可用 `./cc-switch-market account` 查看当前登录 email、session 状态和推导出的 market 公网地址。`./cc-switch-market logout` 会删除本地 router session；如果 market server 正在运行，logout 会拒绝执行并提示先停止进程。

启动后如果 `MARKET_TUNNEL_ENABLED=true`，market 会调用 router 的 `POST /v1/markets/tunnel/lease` 获取 SSH 凭证，并建立反向 tunnel。router 根据 market 登录 session email 识别 market 身份，不再需要配置 `CC_SWITCH_ROUTER_MARKETS` 或 market token hash。

## Web 用户认证

market 的用户身份来源是 router Resend 邮箱验证码，不再集成 Clerk，也不需要可信前置层注入身份头。认证边界：

```text
浏览器
  -> cc-switch-market /login
  -> market 使用本地 web-auth installation 签名请求 router 发送验证码
  -> router Resend 邮箱验证码校验成功
  -> market 签发 HttpOnly session cookie
```

实现要点：

- 浏览器永远不持有 router access token / refresh token。
- market 不信任 `x-clerk-*`、`x-user-*`、`x-admin` 等外部身份头。
- provider claim 余额按当前登录 session email 匹配 router share 的 `owner_email` / `installation_owner_email`。
- admin 权限按 `MARKET_ADMIN_EMAILS` 白名单判定。
- OpenAI / Anthropic 兼容 API 的机器调用仍使用 market 自己签发的 API key，不使用 Web session。

相关文件：

```text
$HOME/.config/cc-switch-market/web-auth-identity.json
```

该文件是 market Web 登录专用的 router installation 私钥，自动生成，权限应保持 `0600`，不要提交到仓库。

## 主要入口

用户侧：

- `/pricing`
- `/dashboard`
- `/usage`
- `/claim`
- `/support`

Admin：

- `/admin`
- `/admin/tickets`
- `/admin/payout-requests`
- `/admin/ledger`
- `/admin/charges`

API：

- `GET /v1/healthz`
- `GET /v1/public/info`
- `POST /v1/api-keys`
- `POST /v1/topups/checkout`
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /responses`
- `POST /v1/messages`
- `GET /v1/provider/claim/summary`
- `POST /v1/provider/claim/payout`
- `POST /v1/provider/claim/payout-ticket`
- `GET /v1/admin/tickets`
- `POST /v1/admin/tickets/{id}/complete-manual-payout`
