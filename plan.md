# cc-switch-market 分阶段实现计划

## 0. 目标与边界

`cc-switch-market` 是面向 `cc-switch` / `cc-switch-router` 的 token 交易市场。market 负责用户门户、充值、余额、价格、路由、扣费、provider 收入记账、提现和运营后台；`cc-switch-router` 负责 tunnel、share 在线状态、share 授权、client secret 保护，并维护 known markets 注册表。

本版规划采用三项外部基础设施：

- **Router Resend 邮箱验证码认证**：API 用户、provider、admin 的 Web 登录统一复用 `cc-switch-router` 的邮箱验证码能力，market 不再集成 Clerk。
- **libSQL / SQLite / Turso**：结构化业务数据与资金 ledger 的唯一事实源。默认本地 SQLite，配置 Turso 后使用远程 Turso + embedded replica。
- **对象存储**：默认本地文件目录 `$HOME/.config/cc-switch-market/objects`，存放 raw webhook、请求/响应调试包、导出文件、结算凭证、工单附件等 blob；Cloudflare R2 作为 V2/生产部署扩展预留。

仍保留 “market 授权代理通道”：

```text
API 用户
  -> cc-switch-market
  -> cc-switch-router 的 market subdomain
  -> router 校验 market 身份与 share 授权
  -> router 注入 client share_token
  -> cc-switch client 本地代理
  -> 上游模型服务
```

关键原则：

- market 不读取、不存储 client 的 `share_token` / API key 明文。
- router 只授予 market 对已授权 share 的使用权，不授予 secret 读取权。
- client 选择 `ForSale=Yes` 并选择某 market 后，cc-switch 自动把该 market email 加入 `shared_with_emails`。
- router 以 `market.email IN share.shared_with_emails` 作为 market 使用某 share 的授权条件。
- router 邮箱验证码是 Web 用户身份源；market DB 以已验证 email 映射用户、provider、admin 业务状态。
- libSQL ledger 是资金事实源；对象存储不是资金事实源，只保存对象和不可变原始材料。
- usage 事实源由 market proxy 自行解析上游响应并写入 `request_charges` / ledger；market 不依赖 router 或 client 的 usage 数据。
- **平台 token 交易抽成可配置**：`MARKET_PLATFORM_COMMISSION_BPS` 控制用户模型消费金额中进入平台收入的比例，默认 `1000`（10%）。充值手续费、提现手续费、FX、chargeback 等外部/通道成本仍独立透传和公开记录，不计入 token 抽成。
- 第一版货币锁定 USD，只支持 Dodo Payments one-time top-up。

V2+ 明确推后：

- 多币种 / FX
- 订阅 / recurring billing
- 多 router 与跨 region 调度
- 更复杂的多渠道 payout 自动化编排（v1 Gate.io 自动提现；其他收款方式走工单 + admin 人工处理）
- self-serve 注销 / GDPR 自动删号
- 数据归档 / 分区 / warehouse
- tokenizer 精确估算
- 地区合规 / 制裁名单
- WebSocket / batch endpoints

## 1. 技术栈

### 1.1 Backend

- Rust edition 2024
- `axum` + `tower` + `tower-http` + `tokio`
- `libsql`，默认本地 SQLite，配置 `libsql://...` 后连接 Turso 远程数据库
- `rust_decimal`，金额与价格计算
- 本地对象存储实现，V2 可增加 `aws-sdk-s3` 或 S3-compatible client 接入 Cloudflare R2
- `reqwest`，调用 router 与 Dodo Payments
- `hmac` + `sha2`，Gate.io API V4 HMAC-SHA512 签名
- `chrono`，Gate.io API timestamp 与审计时间
- `sha2` + `subtle`，API key / session token hash 比对
- `governor`，进程内限流
- `serde` / `serde_json`
- 轻量 `GET /docs` JSON 接口清单；OpenAPI / `utoipa` 作为后置增强
- `tracing` / `tracing-subscriber`

### 1.2 Frontend

- Next.js 15+ App Router + TypeScript
- Tailwind CSS + shadcn/ui
- TanStack Query
- `react-hook-form` + `zod`
- MVP 使用前端本地字典；后续如需扩展再迁移到 `next-intl`
- Playwright + Vitest

### 1.3 Web 邮箱验证码认证依据

market Web 登录完全替换 Clerk，复用 router 已实现的 Resend 邮箱验证码接口：

```text
POST /v1/installations/register
POST /v1/auth/email/request-code
POST /v1/auth/email/verify-code
POST /v1/auth/session/refresh
GET  /v1/auth/session/me
```

router `request-code` 要求 installation 签名，因此浏览器不能直接调用 router。market 后端维护一个专用 `web-auth installation identity`，浏览器只调用 market auth API；market 使用该 installation 私钥签名后转发到 router。router 验证邮箱验证码后返回已验证 email，market 再签发自己的 HttpOnly opaque session cookie。

认证边界：

- 浏览器永远不持有 router access token / refresh token。
- market 不接收也不信任浏览器传入的 `x-clerk-*`、`x-admin` 或 email header。
- market session cookie 只保存随机 session id，服务端 DB 保存 hash、email、过期时间和 admin 判定。
- admin 判定使用 `MARKET_ADMIN_EMAILS` 白名单，后续 V2 可扩展为 DB role / router org role。

## 2. 总体模块

```text
cc-switch-market/
  Cargo.toml
  crates/
    market-server/        main、配置、任务、graceful shutdown
    market-api/           axum routes、middleware、OpenAPI
    market-auth/          router email session、API key、admin 判定
    market-db/            libSQL 连接、SQLite schema、Turso replica/backup、事务工具
    market-object-store/  本地对象存储、object key 规范；V2 扩展 R2
    market-wallet/        ledger、余额、预授权状态机
    market-payments/      Dodo checkout、webhook
    market-pricing/       统一消费/收入价格、模型映射、价格快照
    market-routing/       share sync、健康、选路
    market-proxy/         OpenAI/Anthropic/Gemini proxy、SSE tee
    market-billing/       settle、扣费、provider 收入记账
    market-settlement/    provider claim、Gate.io 提现、结算批次
    market-support/       工单、附件、admin 处理流
    market-router-client/ router API client
    market-web-auth/      router Resend 邮箱验证码代理、本地 Web session
    market-types/         DTO、错误码、共享类型
  web/
```

第一版可以单 crate + 模块化目录，接口稳定后再拆 workspace。

## 3. 执行优先级与后置增强

本版计划主体只保留 MVP 主链路和产品闭环。设计规范、过渡性运营入口和跨 repo 抽象统一降为后置增强，不阻塞当前实现。

### 3.1 当前主线

P0 - 主链路修复：

- client share `app_type` 与 market 选路口径对齐。
- market 选路支持 `parallel_limit = -1` 的无限并发 share。
- market 启动后自动 / 定时同步 router shares，而不是只依赖 admin 手工 sync。
- router `/_market/proxy` 链路改为双向流式透传，消除 `to_bytes(body, usize::MAX)` 带来的大包和流式阻塞风险。
- router host / subdomain / authority 规范化口径统一，避免带端口 host 与 market tunnel domain 比较不一致。

P1 - 产品闭环：

- client 通过 router `GET /v1/markets` 选择 market。
- client 将 market email 自动加入 `shared_with_emails` 并同步 share 到 router。
- client 显示 `Claim earnings on market` 链接，跳转到 market `/claim`。
- provider 收益查看、提现、工单都收敛到 market Web，不在 client 中做 market 收益侧 UI。

P2 - 资金与运营闭环：

- Router Resend Web auth、API key、Dodo top-up、usage 计费、ledger、provider claim、Gate.io payout、support / admin 工作台形成闭环。

P3 - 后置增强：

- usage parser / calculator 的跨 repo 统一与共享库抽取。
- OpenAPI / SDK 文档化。
- client 侧 market 价格展示增强。
- 公告后台化。
- 普通成功请求的对象存储全量留档、导出与更细归档策略。

### 3.2 后置增强：Usage 统一与共享测试向量

目标保持不变：复用 `cc-switch` 已有 usage parser 与 cost calculator，但这项不再作为当前主线阻塞项。

来源：

- `cc-switch/src-tauri/src/proxy/usage/parser.rs`
- `cc-switch/src-tauri/src/proxy/usage/calculator.rs`

推荐落地方式：

- v1 先统一协议测试样例、usage JSON 样本和 cost 计算 golden cases。
- 不再假设三个 repo 处于同一个 workspace，也不要求 sibling path 依赖。
- 如后续确实需要共享 crate，再单独抽成独立 repo 或稳定子模块，而不是在当前阶段强行绑定发布节奏。

## 4. Phase 1 - Router Market Proxy 与 Market Registry

目标：router 支持 market 授权代理通道，并作为 known markets 注册表。

### 4.1 Router Market 身份与 Registry

认证决策：market 不再使用 `ROUTER_MARKET_TOKEN`。market 启动前必须执行一次：

```text
cc-switch-market login
```

登录流程复用 cc-switch client 的 share 邮箱验证码登录：

1. market 生成 / 读取本地 installation identity。
2. market 调 router 邮箱验证码接口。
3. 用户输入验证码后，router 颁发 access token / refresh token。
4. market 将登录邮箱、refresh token、access token 过期时间、router base domain、installation id 保存到 `$HOME/.config/cc-switch-market/router-session.json`，文件权限 `0600`。
5. market 启动时 refresh router session，所有 router market API 使用该 access token。

router 通过 session email 识别 market 身份。`MARKET_EMAIL` 不再作为 env 配置存在，market email 只能来自 `cc-switch-market login` 后的 router session，避免 env email 与实际登录 email 不一致。邮箱登录只证明“请求来自某个 email”；market 权限来自 router DB 中的 market registry。MVP 为最大开放度，market 启动时自动注册 / 续期 registry，注册成功即 active + listed。

market registry 存 router DB，不再依赖 env token hash：

```text
router_markets
id text primary key
display_name text not null
email text not null unique
subdomain text not null unique
public_base_url text not null
scopes_json text not null default '["market:shares:read","market:proxy:use"]'
status text not null default 'active' -- active/offline/disabled
listed integer not null default 1
created_at text not null
updated_at text not null
last_seen_at text not null
offline_since text
```

market 离线保护期：

- router 通过 market tunnel route / lease / heartbeat 更新 `last_seen_at`。
- market 掉线后不立即释放 subdomain，进入 24h 保护期。
- 保护期内 status 可显示为 `offline`，但 subdomain 仍归原 email，其他 market 注册同名 subdomain 必须返回 409。
- `GET /v1/markets` MVP 仍返回 `active + listed` market；offline market 可在 dashboard 显示 offline。
- 定时 cleanup 每 N 分钟扫描 `status=offline AND offline_since < now - 24h` 的 market，删除或归档 registry，释放 subdomain。
- `disabled` market 永不自动释放，需要 admin 手动处理。

market 启动默认动作：

```text
1. 获取 server 进程锁，写入 pid 文件
2. 读取 router-session.json
3. refresh router session
4. 从 session 中取得 market email
5. POST /v1/markets/register
6. body: { subdomain, displayName, publicBaseUrl }
7. router 从 session email 取 market email，禁止请求体覆盖 email
8. 若 email 已存在：允许同 email 更新 displayName/publicBaseUrl，并可续期自己的 subdomain
9. subdomain 变更必须目标 subdomain 未被其他 market 占用，包括 offline 24h 保护期内的 market
10. 若 subdomain 已被其他 email 占用或处于保护期：409
11. 成功后 status=active, listed=true, last_seen_at=now, offline_since=null
```

V2 再增加 approve、risk、listed 审核；MVP 不做 admin approve。

### 4.1.1 Market CLI 与本地账号状态

market CLI 命令：

```text
cc-switch-market login
cc-switch-market account
cc-switch-market logout
cc-switch-market config
cc-switch-market help
```

`cc-switch-market login`：

- 交互输入 email，并通过 router 邮箱验证码登录。
- 登录成功后保存 `$HOME/.config/cc-switch-market/router-session.json`。
- 重复执行 login 会覆盖旧 session，market 下一次启动使用新 email 自动注册 / 续期。
- session 文件权限必须是 `0600`；配置目录不存在时自动创建。

`cc-switch-market account`：

- 显示当前登录 email、session 是否存在、access token 是否过期、refresh 是否可用。
- 显示 `ROUTER_BASE_DOMAIN`、`ROUTER_MARKET_SUBDOMAIN`、推导出的 public URL。
- 若能 refresh，则显示 router session 当前有效；若 refresh 失败，提示重新 login。
- 默认不打印 access token / refresh token；V2 可加 `--json` 给运维脚本使用。

`cc-switch-market logout`：

- 删除本地 router session；若 router 提供 revoke endpoint，则先 revoke refresh token。
- logout 前必须判断 market server 是否正在运行。
- 判断方式使用 `$HOME/.config/cc-switch-market/cc-switch-market.lock` 的 exclusive lock，而不是仅依赖 pid 文件。
- server 启动时持有 lock 并写 `$HOME/.config/cc-switch-market/cc-switch-market.pid`；退出时尽量清理 pid。
- logout 获取 lock 失败时拒绝执行，并提示先停止 market 进程。
- logout 获取 lock 成功但发现 stale pid 时清理 pid，然后删除 session。

### 4.2 Router API

公开 market registry：

```text
GET /v1/markets
```

返回 `status=active AND listed=true` 的 markets，不包含 scopes。

market 自注册 / 续期：

```text
POST /v1/markets/register
Authorization: Bearer <router_access_token>
```

注册成功后，market 才能继续调用 `/v1/market/shares`、`/v1/markets/tunnel/lease` 和 `/_market/proxy/...`。

market 查询可用 share：

```text
GET /v1/market/shares
Authorization: Bearer <router_access_token>
```

过滤：

```text
share.for_sale == "Yes"
AND share.share_status == "active"
AND share.is_online == true
AND market.email IN share.shared_with_emails
```

返回不包含 `share_token`。

market proxy：

```text
ANY https://{market_subdomain}.{router_domain}/_market/proxy/{share_id}/{path...}
Authorization: Bearer <router_access_token>
```

router 校验 router session、market subdomain、market registry status/scopes、share 授权，然后转发到 client tunnel backend，并注入：

```http
X-Share-Token: <share.share_token>
X-CC-Switch-Request-Id: <trace_id>
```

market HTTP 入口：

```text
POST https://{router_domain}/v1/markets/tunnel/lease
Authorization: Bearer <router_access_token>
```

router 根据 session email 找到 registry 中的 active market，只允许该 market 申请自己配置的 `subdomain`。返回短期 SSH lease：

```text
ssh_addr
ssh_username
ssh_password
ssh_host_fingerprint
tunnel_url = https://{market_subdomain}.{router_domain}
```

market 启动后使用该 lease 建立 SSH reverse tunnel，把 `https://{market_subdomain}.{router_domain}` 转发到本地 `MARKET_HTTP_ADDR`。因此：

- `https://{market_subdomain}.{router_domain}/...` 进入 market Web/API。
- `https://{market_subdomain}.{router_domain}/_market/proxy/{share_id}/...` 被 router 内部 route 优先拦截，转发到 provider client tunnel。

market HTTP tunnel lease 使用同一套 router session / market registry scopes，要求 scope 包含 `market:proxy:use`。

### 4.3 Router 转发约束

- reserved subdomain：所有 market subdomain 不能被 client claim。
- reserved path：`/_market`、`/_share-router`、`/_portr`、router 根域 `/v1`。
- market proxy 转发到 client backend 后必须允许 `/v1/messages`、`/v1/chat/completions` 等原始 API path。
- market HTTP 入口的普通路径走 subdomain catch-all proxy，`/_market/proxy` route 必须先于 catch-all。
- market tunnel lease 的 `installation_id` 可使用 `market:{market_id}`，`tunnel_type = market-http`，不带 `share_token`。
- MVP 只要求 `/_market/proxy` 链路实现双向流式透传，消除 `to_bytes(body, usize::MAX)`；不额外抽象成通用转发框架。

### 4.4 Provider Owner Email

provider 收益归属以 router share 的 `owner_email` / `installation_owner_email` 为准。market 不再要求 cc-switch client 调用独立的 ed25519 payment-profile 接口；provider 通过 router 邮箱验证码使用同一个 email 登录 market `/claim` 页面主动提现。

router `/v1/market/shares` 必须返回：

```text
share_id
installation_id
owner_email
installation_owner_email
```

market 仅对 `owner_email == 当前 market Web session email` 或 `installation_owner_email == 当前 market Web session email` 的收益开放 claim。写入 `request_charges.owner_email` 时使用 canonical owner：优先 `owner_email`，为空时 fallback 到 `installation_owner_email`。

## 5. Phase 2 - Market 基础设施

### 5.1 环境变量

```env
MARKET_HTTP_ADDR=0.0.0.0:8080
MARKET_TUNNEL_ENABLED=true

MARKET_SESSION_COOKIE_NAME=cc_switch_market_session
MARKET_SESSION_COOKIE_SECRET=change-me-32-bytes-minimum
MARKET_SESSION_TTL_SECS=2592000
MARKET_ADMIN_EMAILS=admin@example.com

MARKET_SQLITE_PATH=
TURSO_DATABASE_URL=
TURSO_AUTH_TOKEN=
TURSO_REPLICA_PATH=
TURSO_SYNC_INTERVAL_SECS=300
TURSO_BACKUP_ENABLED=true
TURSO_BACKUP_INTERVAL_SECS=3600
TURSO_BACKUP_RETENTION_DAYS=7

OBJECT_STORE_BACKEND=local
OBJECT_STORE_LOCAL_DIR=
R2_ACCOUNT_ID=
R2_ACCESS_KEY_ID=
R2_SECRET_ACCESS_KEY=
R2_BUCKET=
R2_PUBLIC_BASE_URL=

ROUTER_BASE_DOMAIN=router.example.com
ROUTER_MARKET_SUBDOMAIN=main-market
MARKET_DISPLAY_NAME=Main Market

MARKET_MIN_REQUEST_BALANCE=1.00
MARKET_PLATFORM_COMMISSION_BPS=1000

DODO_WEBHOOK_SECRET=...

GATEIO_API_BASE=https://api.gateio.ws
GATEIO_API_KEY=...
GATEIO_API_SECRET=...
GATEIO_SETTLEMENT_CURRENCY=USDT
GATEIO_USD_USDT_RATE=1.000000
GATEIO_SETTLEMENT_ACCOUNT=spot
GATEIO_AUTO_PAYOUT_ENABLED=true
GATEIO_PAYOUT_WORKER_INTERVAL_SECS=60
```

启动期校验：

- `MARKET_SESSION_COOKIE_SECRET` 长度和强度合格；`MARKET_ADMIN_EMAILS` 格式合法。
- 默认本地 SQLite 可创建并可写；配置 Turso 时必须使用 `TURSO_DATABASE_URL=libsql://...` + `TURSO_AUTH_TOKEN`，远程连接失败直接 fail-fast，不回退本地 SQLite。
- Turso 模式使用本地 embedded replica，并每小时备份到 `$HOME/.config/cc-switch-market/turso-db-backup`，仅保留最近 7 天。
- 对象存储默认使用本地目录，并在 `/v1/healthz` 做可写探针；`OBJECT_STORE_BACKEND=r2` 当前只保留配置位，未实现前启动应 fail-fast。
- router session self-check：读取本地 `router-session.json`，refresh 失败或文件不存在则提示执行 `cc-switch-market login`。
- market email 从 router session 动态派生，禁止通过 env 覆盖；`MARKET_EMAIL` 不再存在。
- market registry self-register：refresh 成功后自动调用 `/v1/markets/register`，用 `ROUTER_MARKET_SUBDOMAIN`、推导出的 public base URL、`MARKET_DISPLAY_NAME` 和 session email 注册 / 续期 market。
- `/v1/market/shares` self-check：注册成功后调用，401/403 提示重新 login，409 提示 subdomain 已被其他 email 占用。
- market tunnel self-check：`MARKET_TUNNEL_ENABLED=true` 时调用 `/v1/markets/tunnel/lease` 并建立 SSH reverse tunnel；失败必须持续重试并告警，生产部署可配置为 fail-fast。
- market offline cleanup：router cleanup 任务将断开但未超过 24h 的 market 标记 offline；超过 24h 后释放 registry/subdomain。
- Gate.io 自动提现启用时，启动期必须校验 key/secret 存在，并调用只读账户接口做 self-check；失败则禁用自动提现并告警，不能影响用户消费和 provider 收益记账。
- Gate.io API path、签名 canonical string、Batch Transfers 字段必须在实现前按 Gate.io API V4 官方文档再次确认；plan 只锁定集成边界，不把第三方接口细节写死。
- v1 ledger 记账币种是 USD；Gate.io 实际打款币种是 USDT，按 `GATEIO_USD_USDT_RATE=1.0` 处理为 USD 等值。若未来支持 FX 或非 1:1 汇率，必须新增 FX ledger event，不能静默改金额。

### 5.2 基础接口

```text
GET /v1/healthz
GET /v1/version
GET /v1/public/info
GET /v1/metrics
GET /docs
```

`/v1/healthz` 至少返回：

```json
{
  "ok": true,
  "database": true,
  "databaseMode": "local_sqlite",
  "databasePath": "$HOME/.config/cc-switch-market/cc-switch-market.db",
  "objectStoreBackend": "local",
  "objectStoreWritable": true,
  "auth": { "routerEmailCode": true, "sessionStore": true },
  "routerSync": { "ok": true, "lastSuccessAt": 1760000000, "lagSecs": 12 },
  "ledgerConsistent": true
}
```

`/v1/metrics` v1 可 JSON，后续可 Prometheus：

- request count / error rate by endpoint
- router share sync success / failure
- route success rate
- reservation drift
- upstream P95 latency
- webhook processed / rejected
- object store put/get failures

### 5.3 错误响应模型

JSON API 默认 OpenAI-compatible：

```json
{
  "error": {
    "type": "invalid_request_error",
    "message": "model is not priced",
    "code": "model_not_priced",
    "param": "model",
    "requestId": "req_..."
  }
}
```

流式响应开始后不能改 HTTP status。中途错误写 SSE error event：

```text
data: {"error":{"type":"api_error","message":"usage missing","code":"usage_missing_after_stream","requestId":"req_..."}}

data: [DONE]
```

### 5.4 对象存储规范

对象存储存放 blob，不做资金事实源。MVP 默认写本地目录，object key 与未来 R2 key 保持一致。

对象存储职责分级：

- 强依赖：Dodo webhook 原文、Gate.io request / response / proof、工单附件、需要人工复核的计费证据。
- 弱依赖：普通成功请求的 `request.json` / `response-meta.json`、导出文件、低风险调试包。此类对象允许 best-effort、按阈值记录或后续降级，不应反向定义为主交易链路的唯一可用性前提。

建议 key 规范：

```text
webhooks/dodo/{event_id}.json
requests/{yyyy}/{mm}/{request_id}/request.json
requests/{yyyy}/{mm}/{request_id}/response-meta.json
settlements/{batch_id}/export.csv
settlements/{batch_id}/proof/{item_id}.json
payouts/{payout_request_id}/proof.json
payouts/{payout_request_id}/gateio-request.json
payouts/{payout_request_id}/gateio-response.json
support/tickets/{ticket_id}/attachments/{attachment_id}/{filename}
money-events/{yyyy}/{mm}/{event_id}.json
backups/turso/{yyyy-mm-dd}/{snapshot_id}.db
admin-audit/attachments/{uuid}
```

所有 object metadata 至少包含：

- `content_sha256`
- `created_at`
- `reference_type`
- `reference_id`

DB 中保存 object key 和 hash，不保存大 payload。R2 仅作为 V2/生产扩展后端，不改变 DB 契约。

## 6. Phase 3 - libSQL Schema 与账本（Ledger）

libSQL 是业务事实源。默认本地 SQLite 文件位于 `$HOME/.config/cc-switch-market/cc-switch-market.db`；配置 `TURSO_DATABASE_URL` 后连接 Turso 远程数据库，并维护本地 embedded replica。schema 只做 additive migration；账本（ledger）上线后不 drop / rewrite。

中文产品文案统一把 `ledger` 翻译为“账本”。代码、表名和 API path 仍保留 `ledger`，例如 `ledger_entries`、`/v1/admin/ledger`。

核心表：

```text
users
web_sessions
api_keys
wallet_accounts
ledger_entries
processed_webhooks
topup_orders
model_prices
fee_policies
price_changes
models
router_endpoints
router_shares
share_health
request_charges
request_idempotency
client_earnings_cache
provider_claim_profiles
payout_requests
settlement_batches
settlement_items
tickets
ticket_messages
ticket_attachments
object_refs
admin_audit
```

### 6.1 users

router 邮箱验证码是身份源，DB 保存业务镜像：

```text
id uuid primary key
email text unique not null
email_verified_source text not null default 'router_resend'
status text not null              -- active / restricted / banned
locale text
email_verified_at timestamptz
last_login_at timestamptz
metadata_json jsonb
created_at timestamptz
updated_at timestamptz
```

用户创建方式：

- Web 登录后，后端从 router `verify-code` 响应得到已验证 email。
- 首次访问 `/v1/me` 或 wallet endpoint 时按 email upsert `users`。
- email 是用户、provider claim、admin 白名单的统一身份键；v1 不支持同一用户多邮箱合并。

### 6.2 api_keys

### 6.2 web_sessions

market Web session 是本地 opaque session，不把 router token 暴露给浏览器：

```text
id uuid primary key
user_id uuid references users(id)
email text not null
session_token_hash text unique not null
router_user_id text
router_access_expires_at timestamptz
expires_at timestamptz not null
last_seen_at timestamptz
last_seen_ip inet
ip_country text
user_agent text
created_at timestamptz
revoked_at timestamptz
```

约束：

- cookie 中只放随机 session token，DB 只存 hash。
- session 过期或 revoked 后必须返回 401。
- admin 权限每次请求按 session email 与 `MARKET_ADMIN_EMAILS` 动态判定，不固化到 cookie。
- v1 不保存 router refresh token；market Web session 到期后重新邮箱验证码登录。

### 6.3 api_keys

LLM API 调用仍使用 market API key，不直接使用 Web session。

```text
id uuid primary key
user_id uuid references users(id)
name text
key_hash text not null
prefix text not null
scope_json jsonb
expires_at timestamptz
monthly_spend_cap numeric
last_used_at timestamptz
last_used_ip_country text
created_at timestamptz
revoked_at timestamptz
```

### 6.4 wallet / ledger

账户类型：

```text
user_cash
user_reserved
client_payable
payment_clearing
settlement_paid
payout_reserved             -- provider 已发起 claim、等待提现执行的锁定余额
risk_loss
fee_revenue                 -- 充值/提现手续费与 token 交易平台抽成，按 ledger event type 区分
```

原则：

- 所有资金变化写 `ledger_entries`。
- 余额缓存存在 `wallet_accounts.balance`，事务内同步。
- 资金事务使用 SQLite/libSQL `BEGIN IMMEDIATE`，所有余额读写必须在同一个事务内完成。
- 每笔 ledger 有 `reference_type`、`reference_id`、`actor_type`、`actor_id`、`client_ip`、`ip_country`。
- client payable 聚合维度：`owner_email`，同 owner 多 share 合并结算。
- 所有与钱有关的变动必须能从公开可见的业务记录追溯到 ledger：充值、手续费、消费、provider 收入、提现、退款、失败回滚都不能只有内部日志。
- provider 发起 claim 时必须立即从 `client_payable` 转入 `payout_reserved`，避免同一收益被重复提现；失败或取消再原路释放。

`wallet_accounts`：

```text
id uuid primary key
account_type text not null        -- user_cash / user_reserved / client_payable / ...
currency text not null default 'USD'
owner_user_id uuid                -- user_cash / user_reserved
owner_email text                  -- client_payable / payout_reserved
balance numeric(20, 8) not null default 0
metadata_json jsonb
created_at timestamptz
updated_at timestamptz
```

SQLite 中账户唯一性使用 partial unique index 实现：

```text
unique(account_type, currency, owner_user_id) where owner_user_id is not null
unique(account_type, currency, owner_email) where owner_email is not null
unique(account_type, currency) where owner_user_id is null and owner_email is null
```

`ledger_entries`：

```text
id uuid primary key
transaction_id uuid not null
from_account_id uuid references wallet_accounts(id)
to_account_id uuid references wallet_accounts(id)
amount numeric(20, 8) not null
currency text not null default 'USD'
reference_type text not null      -- topup / request_charge / payout_request / refund / adjustment
reference_id uuid not null
actor_type text not null          -- system / user / provider / admin / webhook
actor_id text
client_ip inet
ip_country text
metadata_json jsonb
created_at timestamptz
```

账本约束：

- `amount > 0`。
- 同一 `transaction_id` 下所有 entries 必须 currency 一致。
- wallet balance 只能通过账本 transaction 更新，禁止业务代码直接改余额。
- 账本 balance check 至少校验：所有账户余额等于 ledger entry 聚合值、`user_reserved` 与 in-flight request 一致、`payout_reserved` 与 pending/processing/needs_review payout 一致。

### 6.5 资金透明视图

所有角色都应能在权限范围内查询“钱从哪里来、到哪里去、依据是什么”：

```text
GET /v1/wallet/ledger              -- API 用户：充值、充值手续费、消费、退款
GET /v1/usage                      -- API 用户：每笔模型消费明细
GET /v1/provider/earnings          -- Provider：每笔收入明细
GET /v1/provider/claim/payouts     -- Provider：每笔提现、手续费、关联工单
GET /v1/tickets                    -- API 用户 / Provider：自己提交的反馈与提现工单
GET /v1/admin/ledger               -- Admin：全局账本
GET /v1/admin/money-events         -- Admin：充值/消费/收入/提现/退款统一事件流
```

公共字段要求：

- 所有金额字段同时显示 `gross_amount`、`fee_amount`、`net_amount`（不适用时 fee 为 0）。
- 所有事件都有 `event_id`、`reference_type`、`reference_id`、`created_at`、`status`。
- request 消费必须显示 `request_id`、model、token usage、单价快照、总额。
- provider 收入必须显示对应 `request_id`，证明收入来自哪次 API 消费。
- 充值/退款必须显示 Dodo payment/refund id 与 raw webhook object key hash。
- 提现必须显示收款方式、收款目标脱敏值、手续费、external tx id、proof object key hash；非 Gate.io 人工提现还必须显示关联 ticket id。

`money_events` 可作为账本的公开查询投影或物化视图：

```text
event_id
event_type                  -- topup / topup_fee / usage_charge / provider_income / platform_commission / payout_reserved / payout / payout_fee / payout_released / refund / manual_adjustment
viewer_user_id              -- API 用户可见范围
viewer_owner_email          -- provider 可见范围
gross_amount
fee_amount
net_amount
currency
reference_type
reference_id
ledger_entry_ids
object_ref_ids
status
created_at
```

约束：`usage_charge.gross_amount == provider_income.net_amount + platform_commission.amount`，否则 ledger balance check 必须失败。

### 6.6 用户侧查询接口字段

`GET /v1/usage`：

```text
query: cursor, limit, time_from, time_to, app_type, status
return: items[], next_cursor
item: request_id, app_type, model, status, input_tokens, output_tokens,
      cache_read_tokens, cache_write_tokens, price_snapshot,
      gross_amount, fee_amount=0, net_amount, currency, created_at, settled_at
```

`GET /v1/wallet/ledger`：

```text
query: cursor, limit, event_type, time_from, time_to
return: items[], next_cursor
item: event_id, event_type, gross_amount, fee_amount, net_amount, currency,
      status, reference_type, reference_id, created_at
```

`GET /v1/api-keys`：

```text
item: id, name, prefix, scope_json, expires_at, monthly_spend_cap,
      last_used_at, last_used_ip_country, created_at, revoked_at
```

所有列表接口 v1 统一 cursor pagination，默认 limit 50，最大 200。

## 7. Phase 4 - Router Resend 邮箱验证码认证与 Web 用户

### 7.1 Next.js

前端实现全局邮箱验证码登录弹窗/组件，不新增独立 `/login` 页面：

- 第一步输入 email，调用 `POST /v1/auth/email/request-code`。
- 第二步输入 6 位验证码，调用 `POST /v1/auth/email/verify-code`。
- 登录成功后后端设置 HttpOnly Secure SameSite=Lax session cookie，前端刷新当前页面数据；如果是从 CTA 触发，可跳转 `/dashboard`。
- 顶部登录状态调用 `GET /v1/me`，不从 localStorage 读取 token。
- `POST /v1/auth/logout` 清除 cookie 并 revoke 本地 session。
- 登录弹窗展示 router Resend cooldown、错误次数过多、验证码过期等错误。

前端不处理密码、重置密码、MFA；邮箱验证码发送、验证码校验和频率限制由 router 统一处理。

### 7.2 Axum

后端保护 Web/session endpoints：

- 启动时生成 / 读取 `web-auth installation identity`，必要时调用 router `/v1/installations/register` 注册。
- `request-code` handler 使用该 installation 私钥签名 `auth_request_code` payload 后转发 router `/v1/auth/email/request-code`。
- `verify-code` handler 转发 router `/v1/auth/email/verify-code`，成功后按 email upsert `users`，创建 `web_sessions`，设置 HttpOnly cookie。
- session middleware 从 cookie 读取 opaque token，hash 后查 `web_sessions`，把 `MarketPrincipal { user_id, email, is_admin }` 注入 request extension。
- admin 判定：
  - v1：`email IN MARKET_ADMIN_EMAILS`。
  - 后端二次校验 session store，不信任前端传参或 header。

### 7.3 Auth 接口变化

不实现密码账号体系：

```text
不再实现 POST /v1/auth/register
不再实现 forgot/reset password
```

market Web auth 接口：

```text
POST /v1/auth/email/request-code
POST /v1/auth/email/verify-code
POST /v1/auth/logout
GET /v1/me
GET /v1/session/status
```

`POST /v1/auth/email/request-code` 入参：

```json
{ "email": "user@example.com" }
```

market 转发 router 时补齐：

```text
installationId
timestampMs
nonce
signature
```

`POST /v1/auth/email/verify-code` 入参：

```json
{ "email": "user@example.com", "code": "123456" }
```

成功响应：

```json
{
  "user": { "id": "uuid", "email": "user@example.com", "isAdmin": false },
  "expiresAt": "2026-05-28T00:00:00Z"
}
```

API key 管理接口要求 Web session：

```text
POST   /v1/api-keys
GET    /v1/api-keys
POST   /v1/api-keys/{id}
DELETE /v1/api-keys/{id}
```

## 8. Phase 5 - Dodo Payments 充值

用户通过 market Web session 创建和查询充值订单；Dodo webhook 是公网回调，只做签名和幂等校验，不要求 Web session。

```text
POST /v1/topups/checkout
GET  /v1/topups/{id}
POST /v1/webhooks/dodo
```

`topup_orders`：

```text
id uuid
user_id uuid
payment_provider text default 'dodo'
provider_payment_id text
gross_amount numeric              -- 用户支付金额
fee_amount numeric                -- 充值通道手续费，公开展示
net_amount numeric                -- 实际入账 user_cash
currency text default 'USD'
status text                 -- pending / paid / expired / refunded / chargeback
checkout_url text
metadata_json jsonb
raw_payload_object_key text
created_at timestamptz
expires_at timestamptz
paid_at timestamptz
refunded_at timestamptz
```

Webhook：

- 验签 + timestamp tolerance。
- event id 幂等。
- raw payload 写对象存储：`webhooks/dodo/{event_id}.json`。
- payment success：
  ```text
  payment_clearing -> user_cash     net_amount
  payment_clearing -> fee_revenue   fee_amount
  ```
- 充值页面和订单详情必须公开显示 `gross_amount`、`fee_amount`、`net_amount`、手续费计算规则和 Dodo payment id。
- refund / chargeback：只冲销用户余额与平台风险账户，不影响已产生 `client_payable`。
- pending 超时任务标记 `expired`。

refund / chargeback ledger：

```text
user_cash -> payment_clearing      min(user_cash_available, refund_net_amount)
risk_loss -> payment_clearing      refund_net_amount - amount_recovered_from_user_cash
```

已收取的充值手续费是否退还由 Dodo 实际退款结果和 `fee_policies` snapshot 决定；如果退还手续费：

```text
fee_revenue -> payment_clearing    refunded_fee_amount
```

`processed_webhooks`：

```text
provider text not null             -- dodo
event_id text not null
event_type text not null
status text not null               -- processed / ignored / failed
raw_payload_object_key text
error_message text
processed_at timestamptz
created_at timestamptz
primary key(provider, event_id)
```

Dodo 未识别事件先写 `processed_webhooks(status=ignored)` 和对象存储原文，不做资金变动；已识别事件必须在同一事务内完成幂等记录和 ledger 更新。

## 9. Phase 6 - 模型、价格与路由运营

接口：

```text
GET /v1/prices                         -- 首页公开价格，只返回 active 且 model_pattern != "*" 的模型
GET /v1/admin/models
POST /v1/admin/models
GET /v1/admin/models/{id}
PATCH /v1/admin/models/{id}
POST /v1/admin/models/{id}/activate
POST /v1/admin/models/{id}/deactivate
PUT /v1/admin/models/{id}/price
GET /v1/admin/models/{id}/price-changes
PUT /v1/admin/models/{id}/routing
PUT /v1/admin/models/{id}/routing/shares
POST /v1/admin/models/route-preview
GET /v1/admin/price-changes
```

匹配算法：

```text
1. (app_type, exact model)
2. (app_type, model_prefix*) 最长前缀
3. (app_type, "*")
4. 否则 400 model_not_supported
```

配置校验：

- 所有 active model 必须有唯一有效价格。
- 下线模型优先级高于 `*` fallback；如果请求模型命中 inactive 规则，必须直接返回 `model_offline`，不能继续 fallback 到 `*`。
- 价格即用户消费价，也是 provider 每 token 收入价。
- 平台不得配置隐藏 token 交易差价；同一次 usage 的用户扣费金额必须等于 provider 净收入加平台抽成。

`fee_policies`：

```text
id
fee_type                    -- topup / payout
method                      -- dodo / gateio
fixed_usd
percent_bps
min_usd
max_usd
currency text default 'USD'
status
effective_from
```

充值手续费、提现手续费都来自 `fee_policies`，必须在用户提交前公开展示，并写入订单 / payout 快照。

`models` 是 admin 运营主实体；价格和路由都是模型的配置项：

```text
id
app_type text not null                 -- openai / anthropic / gemini
model_pattern text not null            -- exact / prefix* / *
display_name
canonical_name
status text not null                   -- active / inactive
is_public boolean default true         -- 历史兼容字段；v1 首页展示由 status 和 model_pattern 自动决定
sort_order integer default 0
aliases_json
metadata_json
created_at
updated_at
unique(app_type, model_pattern)
```

- admin 通过“模型”入口增删改模型、上下线模型、改价格、设置可路由 share。
- 首页价格只展示 `models.status=active` 且 `model_pattern != '*'` 的模型；`*` 规则永远不在首页展示，只作为内部兜底。
- v1 不做 alias rewrite，`aliases_json` 仅预留。

`model_prices`：

```text
id uuid primary key
model_id uuid not null references models(id)
input_per_million numeric(20, 8) not null
output_per_million numeric(20, 8) not null
cache_read_per_million numeric(20, 8) not null default 0
cache_write_per_million numeric(20, 8) not null default 0
currency text not null default 'USD'
status text not null              -- active / inactive
effective_from timestamptz not null
created_at timestamptz
updated_at timestamptz
```

`price_changes` 必须保存 old/new snapshot、admin actor、reason，保证历史请求按当时价格可复算。

模型上下线：

- `deactivate` 只影响新请求，不影响历史账单、历史收益和已生成的 price snapshot。
- 下线模型不再出现在首页价格中，也不再参与路由；上线的非 `*` 模型自动出现在首页价格中。
- 上线模型必须先通过配置校验：存在 active price，路由规则不会导致显式 0 候选，或 admin 确认强制上线。
- 上线、下线、改价、改路由都必须写 `admin_audit`。

## 10. Phase 7 - Router Share Sync 与选路

market 定时调用：

```text
GET /v1/market/shares
Authorization: Bearer <router_access_token_from_router-session.json>
```

写 `router_shares`：

```text
router_id
share_id
installation_id
owner_email
installation_owner_email
app_type
for_sale
share_status
online
active_requests
parallel_limit
online_rate_24h
priority integer default 0
raw_json jsonb
last_seen_at
failure_count integer default 0
cooldown_until timestamptz
unique(router_id, share_id)
```

写 `model_routing_rules`：

```text
id
model_id references models(id)
mode                             -- all / include_only / exclude
priority integer default 0
enabled boolean default true
notes
created_at
updated_at
unique(model_id)
```

写 `model_routing_rule_shares`：

```text
rule_id
router_id
share_id
created_at
primary key(rule_id, router_id, share_id)
```

模型路由规则：

- 默认没有规则时等价于 `mode=all`，即该模型所属 `app_type` 下所有符合基础条件的 share 都可路由。
- admin 在“模型”详情中为某个模型设置 `exclude`，从候选 share 中排除指定 share。
- admin 在“模型”详情中设置 `include_only`，让该模型只路由到指定 share。
- `include_only` / `exclude` 都只影响最终候选集，不改变价格、不改变 provider 收益计算。
- 模型匹配先命中 `models`：精确模型名 > `prefix*` > `*`；命中 active 模型后再读取该模型的路由规则。
- MVP 不做多规则叠加；同一模型只允许一条 enabled 路由规则。V2 再扩展多规则组合、质量评分和供应商分层。

v1 单 router：`router_id = main`。V2 使用 `router_endpoints`。

选路：

```text
app_type match
model routing rule match
online = true
active_requests < parallel_limit
share_status = active
for_sale = Yes
coalesce(owner_email, installation_owner_email) is not null
not cooling down
```

选路流程：

1. 从 endpoint 推导 `app_type`，从请求 body 或 Gemini URL 推导 `model`；不信任用户 body 中覆盖 `app_type` 的字段。
2. 匹配 `models`；没有匹配返回 `model_not_supported`。
3. 如果命中模型为 inactive，返回 `model_offline`，不能 fallback 到 `*`。
4. 读取模型 active price 和 routing rule；没有 routing rule 则使用隐式 `all`。
5. 先按基础条件生成候选 share。
6. `mode=all` 不改变候选集。
7. `mode=exclude` 排除规则绑定的 share。
8. `mode=include_only` 只保留规则绑定的 share。
9. 过滤后为空时返回 `no_route_for_model`，错误信息包含 `app_type`、`model`、`model_id`、`matched_rule_id`，方便 admin 排查。
10. 对剩余候选排序：低 active requests、高 priority、高 online rate、最近成功、随机打散。

路由审计：

- `request_charges` 记录 `model_id`、`routing_rule_id`（无显式规则时为空）、`router_id`、`share_id`、`owner_email`。
- 增加 `request_attempts`，记录每次尝试的 share、失败类型、冷却结果、latency 和最终选中原因。
- Admin 提供路由预览接口，输入 `app_type + model`，返回命中的模型、规则、基础候选、因并发/冷却/blocklist/规则被排除的 share、最终候选和当前会选择的 share。

失败重试与冷却：

- `select_share` 演进为 `select_share_candidates(model_id, app_type, model, limit)`，返回有序候选列表。
- 非流式请求在同一笔预授权内最多尝试 3 个 share；全部失败才释放预授权。
- 流式请求只在 router 返回非 2xx 或连接失败、尚未向用户发送 body 前允许换 share；一旦开始输出 SSE chunk，就不能透明重试，只能写 SSE error 并按现有 needs_review / release 流程处理。
- `cooldown_until + failure_count` 替代硬编码 `last_error_at + 2min`：
  - 第 1 次失败：30 秒。
  - 第 2 次失败：2 分钟。
  - 第 3 次失败：5 分钟。
  - 第 4 次及以后：15 分钟封顶。
- 成功后清零 `failure_count`、`cooldown_until`、`last_error_at`、`last_error_message`。
- 可重试错误：network、timeout、429、502、503、504。
- 非重试或长冷却错误：401、403、模型不支持、share 授权失败。

模型/share 自动 blocklist：

```text
model_share_blocks
model_id
router_id
share_id
reason
expires_at
created_at
primary key(model_id, router_id, share_id)
```

- 如果某 share 明确返回模型不存在/不支持，将该 `model_id + share` 临时加入 blocklist。
- blocklist 只影响路由候选，不影响历史账单和 provider 收益。
- Admin route preview 必须显示被 blocklist 排除的 share；V2 再提供手动解除入口。

## 11. Phase 8 - Market API Proxy、预授权、幂等

入口：

```text
POST /v1/chat/completions
POST /v1/messages
```

LLM API 鉴权：

- `Authorization: Bearer sk-cs-...`
- API key hash 查 `api_keys`。
- Web session 不用于机器调用。

状态机：

1. 验 API key。
2. 解析 app_type / model。
3. 匹配统一 token 价格。
4. 计算 `request_body_hash`。
5. Idempotency:
   - finalized + same body hash → replay charge 摘要。
   - same key + different body hash → 409。
   - in_progress → 409/425。
6. 预授权：
   - `estimated_input_tokens = ceil(body_bytes / 2)`。
   - `reserved_amount = estimated_input_tokens * price.input + max_output_tokens * price.output`。
   - 要求余额 ≥ `max(reserved_amount, MARKET_MIN_REQUEST_BALANCE)`。
   - `user_cash -> user_reserved`。
7. 选 route share。
8. 转发 router market proxy。
9. SSE tee 解析 usage。
10. settle。

流式中途错误：

- HTTP status 已发出后不能修改。
- 写 SSE error event + `[DONE]`。
- 释放预授权或按已解析 usage 计费，按失败矩阵处理。

`request_idempotency`：

```text
user_id
idempotency_key
request_body_hash
charge_id
status                 -- in_progress / finalized / failed_released
created_at
completed_at
unique(user_id, idempotency_key)
```

## 12. Phase 9 - 扣费与 Provider 收入

计费：

```text
billable_input_tokens = input_tokens - cache_read_tokens

usage_amount =
  billable_input_tokens * price.input
  + output_tokens * price.output
  + cache_read_tokens * price.cache_read
  + cache_write_tokens * price.cache_write
```

成功 settle：

```text
user_reserved -> user_cash         reserved_amount - usage_amount
user_reserved -> client_payable    provider_net_amount
user_reserved -> fee_revenue       platform_commission_amount
```

`platform_commission_amount = usage_amount * MARKET_PLATFORM_COMMISSION_BPS / 10000`，默认 10%。`provider_net_amount = usage_amount - platform_commission_amount`。充值、提现、FX、chargeback 成本仍通过各自 fee policy / reserve 单独处理，不与 token 抽成混算。

如果 `usage_amount > reserved_amount`：

- 优先在同一事务中从 `user_cash` 补扣差额，并按同一抽成比例拆入 `client_payable` / `fee_revenue`。
- 如果 `user_cash` 不足，已实际收到的部分按抽成比例拆账；未收到差额从 `risk_loss` 承担并转入 `client_payable`，不给平台形成未收款抽成收入；同时给 `request_charges.audit_flags` 写 `settlement_over_reserved`。
- 不能因为预授权不足减少 provider 收入；估算失败属于 market 风险。

失败 / 拒结算：

```text
user_reserved -> user_cash         reserved_amount
```

`request_charges` 保存价格快照、usage、share、owner_email、status、audit_flags 和 `usage_amount`。大 payload 或调试包写对象存储，并在 DB 中存 object key。

`request_charges`：

```text
id uuid primary key
request_id text unique not null
user_id uuid not null
api_key_id uuid not null
router_id text not null
share_id text not null
owner_email text not null
app_type text not null
model text not null
status text not null              -- reserved / streaming / settled / failed_released / failed_charged / needs_review
idempotency_key text
request_body_hash text
reserved_amount numeric(20, 8) not null
usage_amount numeric(20, 8)
price_snapshot jsonb not null
usage_json jsonb
audit_flags jsonb
request_object_key text
response_meta_object_key text
created_at timestamptz
settled_at timestamptz
```

公开查询要求：

- API 用户可在 `/usage` 看到每笔消费的 token、单价、总额、请求状态、对应 request id。
- Provider 可在 `/claim` 看到每笔收入明细：request id、model、token、单价、收入金额、结算状态。
- Admin 可看到用户消费与 provider 收入的一一对应关系，方便核对“用户扣了多少，provider 应收多少”。

## 13. Phase 10 - Provider Claim 与提现

provider 不在 cc-switch 中填写收款信息。provider 使用 share owner email 登录 market Web `/claim`，查看待提现余额；待提现余额 ≥ 1 USD 时，可以选择 Gate.io UID/email 自动提现，或选择其他收款方式创建提现工单，由 admin 人工处理。

身份规则：

- provider 使用 router 邮箱验证码登录 market。
- 当前 market Web session email 必须等于 router 返回的 `owner_email` 或 `installation_owner_email`。
- market 根据 `request_charges.owner_email` / ledger 中 `client_payable` 账户汇总该 email 的收益。
- cc-switch 不需要接 market Web auth，也不需要提供收款信息表单。

Web/API：

```text
GET  /v1/provider/claim/summary
POST /v1/provider/claim/payout
GET  /v1/provider/claim/payouts
POST /v1/provider/claim/payout-ticket
```

`GET /v1/provider/claim/summary` 返回：

```json
{
  "ownerEmail": "provider@example.com",
  "availableUsd": "12.34",
  "pendingUsd": "0.00",
  "paidUsd": "100.00",
  "minimumPayoutUsd": "1.00",
  "canPayout": true
}
```

`POST /v1/provider/claim/payout` 入参：

```json
{
  "method": "gateio",
  "params": { "email": "seller@example.com", "uid": null },
  "amountUsd": "12.34",
  "feeUsd": "0.20",
  "netPayoutUsd": "12.14"
}
```

`POST /v1/provider/claim/payout-ticket` 入参：

```json
{
  "method": "other",
  "amountUsd": "12.34",
  "feeUsd": "0.20",
  "netPayoutUsd": "12.14",
  "payoutDetailsText": "USDT TRC20 address: ...",
  "attachmentIds": ["att_..."]
}
```

校验：

- `availableUsd >= 1.00`。
- `amountUsd <= availableUsd`。
- Gate.io 自动提现：`params.email` 与 `params.uid` 至少一项必填。
- 其他收款方式：`payoutDetailsText` 或附件至少一项必填，创建 `ticket_type=payout_manual` 工单。
- 同一 owner 同时只能有一个 `pending` / `processing` / `needs_review` payout request。
- 提交前必须展示提现手续费：`gross amount`、`feeUsd`、`netPayoutUsd`，并将 fee policy 快照写入 `payout_requests`。
- `availableUsd` 来自 ledger 中该 owner 的 `client_payable` 余额，公开明细来自 `request_charges`，两者必须可对账。
- 创建 payout request 与锁定余额必须在同一 DB 事务内完成：
  ```text
  client_payable -> payout_reserved amount_usd
  ```
- provider 页面显示的 `pendingUsd` 来自 `payout_reserved`，不是只查 `payout_requests.status`。
- 其他收款方式创建工单时也必须先创建 `payout_requests(method=manual, status=pending)` 并锁定 `payout_reserved`；admin 不能通过直接改余额完成提现。

`provider_claim_profiles`：

```text
owner_email primary key
method text                 -- gateio / other
params_json jsonb           -- gateio: {email, uid}; other: free-form payout profile
updated_at
```

`payout_requests`：

```text
id uuid primary key
owner_email text not null
amount_usd numeric not null
payout_fee_usd numeric not null
net_payout_usd numeric not null
method text not null
params_json jsonb not null
fee_policy_snapshot jsonb
ticket_id uuid
status text                  -- pending / processing / needs_review / paid / failed / cancelled
settlement_batch_id uuid
settlement_item_id uuid
external_tx_id text
proof_object_key text
gateio_batch_id text
gateio_request_object_key text
gateio_response_object_key text
failure_reason text
created_at timestamptz
processing_at timestamptz
paid_at timestamptz
failed_at timestamptz
cancelled_at timestamptz
```

## 14. Phase 11 - Gate.io 自动提现与结算执行

provider 在 `/claim` 发起 `payout_request` 后，后台任务触发 Gate.io batch transfer。MVP 为降低幂等和对账复杂度，按 “一个 `payout_request` 一次 Gate.io Batch Transfers 调用，batch 内一个 item” 执行；V2 再把多笔 request 聚合到同一个 batch。触发入口来自 provider claim，不再要求 admin 主动扫描所有 payable 自动打款。admin 只负责复核、重试和处理失败单。

接口：

```text
GET  /v1/admin/settlements
GET  /v1/admin/payout-requests
POST /v1/admin/payout-requests/{id}/execute-gateio
POST /v1/admin/payout-requests/{id}/mark-paid
POST /v1/admin/payout-requests/{id}/mark-failed
POST /v1/admin/payout-requests/{id}/cancel
```

提现执行：

- provider claim 创建 `payout_requests(status=pending)`，并已把 gross amount 从 `client_payable` 锁定到 `payout_reserved`。
- 后台 worker 每 `GATEIO_PAYOUT_WORKER_INTERVAL_SECS` 扫描 pending request；admin 也可通过 `execute-gateio` 对单笔 request 触发同一执行逻辑。
- 执行逻辑在 DB 事务中用 `FOR UPDATE SKIP LOCKED` 获取 request，将 `pending -> processing`；并发 worker / admin 不能重复执行。
- 调 Gate.io API V4 现货账户批量转账（Batch Transfers），目标为 provider 填写的 Gate.io UID 或 email。
- Gate.io API 认证使用 Key + Secret HMAC-SHA512 签名；签名逻辑可参考官方 Rust SDK 的 Auth 实现。
- Rust 实现使用：
  - `reqwest` 发送异步 HTTP 请求。
  - `serde` 处理请求/响应 JSON。
  - `chrono` 生成 API timestamp。
  - `hmac` + `sha2` 生成 HMAC-SHA512。
- Gate.io request / response 原文写对象存储：
  ```text
  payouts/{payout_request_id}/gateio-request.json
  payouts/{payout_request_id}/gateio-response.json
  ```
- Gate.io 返回成功后，保存 `gateio_batch_id` / `external_tx_id`，凭证 JSON 写对象存储：`payouts/{payout_request_id}/proof.json`。
- Gate.io 返回成功后 ledger：
  ```text
  payout_reserved -> settlement_paid net_payout_usd
  payout_reserved -> fee_revenue    payout_fee_usd
  ```
  `net_payout_usd = amount_usd - payout_fee_usd` 是实际转给 provider 的金额。
- Gate.io transfer item amount 使用：
  ```text
  transfer_amount_usdt = net_payout_usd / GATEIO_USD_USDT_RATE
  ```
  MVP 固定 `GATEIO_USD_USDT_RATE=1.0`；Gate.io request/proof 中同时保存 USD ledger amount 与 USDT transfer amount，方便对账。
- Gate.io 明确失败或 admin `mark-failed` 后，request 进入 `failed` 终态并释放锁定余额：
  ```text
  payout_reserved -> client_payable amount_usd
  ```
- 网络超时 / 结果未知不能直接释放余额，必须保持 `processing` 或进入 `needs_review` 扩展状态，admin 根据 Gate.io 查询结果处理，避免重复打款。
- `cancel` 只允许 pending request，进入 `cancelled` 终态，释放锁定余额并记录 actor/reason。
- Gate.io 自动提现结果未知时保留人工 fallback：admin 可在外部确认或完成转账后用 `mark-paid` 填入 `external_tx_id` 和 proof；`mark-paid` 只能从 `processing` / `needs_review` 执行，且必须复用同一笔 `payout_reserved` 做 ledger 结算，不能重新扣 `client_payable`。
- `failed` / `cancelled` 都是已释放余额的终态；provider 如需提现必须重新提交 payout request。

公开查询要求：

- Provider 可在 `/claim` 查看每笔 payout request 的 `amountUsd`、`feeUsd`、`netPayoutUsd`、Gate.io 目标、状态、Gate.io batch id / external tx id、时间。
- Admin 可按 owner email / payout status / Gate.io batch id / external tx id 查询。
- 每笔提现必须能追溯到 request_charges 收入明细、ledger entries、Gate.io request/response、proof object。
- `payout_requests` 详情页必须展示状态流转时间线：created、processing、paid/failed/cancelled、操作者、失败原因、object hash。

幂等与安全：

- `payout_requests.id` 作为 Gate.io batch transfer 的客户端幂等 reference（若 Gate.io 接口支持自定义 text/memo/client id，则必须填入）；V2 聚合多笔时改用 `settlement_batches.id` 作为 batch reference，并把 `payout_requests.id` 写到 item memo。
- 同一 payout request 只能执行一次成功转账；状态机必须在 libSQL `BEGIN IMMEDIATE` 事务内用状态条件更新防并发执行。
- Gate.io API key/secret 只存在服务端 secret 环境，不进入 DB、对象存储、日志。
- 日志中 Gate.io key、签名、完整收款目标必须 mask；provider 页面只显示目标脱敏值。

## 15. Phase 12 - 工单系统与人工处理

目标：支持用户反馈工单、provider 非 Gate.io 收款方式提现工单、文字和图片附件、admin 处理流。v1 附件仅支持图片。工单可以承载人工沟通，但凡涉及资金变化，必须通过 ledger transaction，不允许直接修改余额缓存。

工单类型：

```text
feedback                 -- 使用反馈 / bug / 需求，不默认关联资金
payout_manual            -- provider 其他收款方式提现，必须关联 payout_request
billing_issue            -- 充值、扣费、退款争议
account_issue            -- 账号 / API key / 风控问题
```

用户 / provider API：

```text
POST /v1/ticket-attachments/presign
POST /v1/tickets
GET  /v1/tickets
GET  /v1/tickets/{id}
POST /v1/tickets/{id}/messages
```

Admin API：

```text
GET  /v1/admin/tickets
GET  /v1/admin/tickets/{id}
POST /v1/admin/tickets/{id}/assign
POST /v1/admin/tickets/{id}/messages
POST /v1/admin/tickets/{id}/status
POST /v1/admin/tickets/{id}/link-payout
POST /v1/admin/tickets/{id}/complete-manual-payout
POST /v1/admin/tickets/{id}/adjust-provider-payable
```

`tickets`：

```text
id uuid primary key
ticket_no text unique not null
ticket_type text not null         -- feedback / payout_manual / billing_issue / account_issue
status text not null              -- open / waiting_user / waiting_admin / resolved / closed
priority text not null            -- low / normal / high / urgent
subject text not null
creator_user_id uuid
creator_owner_email text          -- provider 工单使用
related_payout_request_id uuid
related_reference_type text
related_reference_id uuid
assigned_admin_id text
metadata_json jsonb
created_at timestamptz
updated_at timestamptz
closed_at timestamptz
```

`ticket_messages`：

```text
id uuid primary key
ticket_id uuid references tickets(id)
sender_type text not null         -- user / provider / admin / system
sender_id text
body_text text not null
internal_note boolean not null default false
created_at timestamptz
```

`ticket_attachments`：

```text
id uuid primary key
ticket_id uuid references tickets(id)
message_id uuid references ticket_messages(id)
uploader_type text not null       -- user / provider / admin
object_key text not null
content_sha256 text not null
content_type text not null
byte_size bigint not null
original_filename text
created_at timestamptz
```

附件规则：

- v1 仅支持图片附件，`content_type` 必须为 `image/*`；图片上传到对象存储，DB 只保存 object key、hash、content type、大小。
- v1 限制单文件 2 MB，单工单最多 10 个附件。
- presigned upload 完成后必须调用创建消息 / 创建工单接口绑定附件；未绑定附件由清理任务删除。
- admin 下载附件必须经过后端鉴权，不直接暴露对象存储 public URL。

普通反馈工单：

- API 用户可在 `/support` 提交文字、图片、相关 request id。
- Provider 可在 `/claim` 或 `/support` 提交收益/提现问题。
- 普通反馈工单不产生 ledger entry；如果 admin 最终需要调账，必须走 `adjust-provider-payable` 或用户调账接口，并在工单时间线显示 ledger reference。

人工提现工单：

- Provider 在 `/claim` 选择“其他收款方式”后提交 `POST /v1/provider/claim/payout-ticket`。
- 后端在同一事务中：
  ```text
  client_payable -> payout_reserved amount_usd
  ```
  并创建 `payout_requests(method=manual, status=pending, ticket_id=...)` 与 `tickets(ticket_type=payout_manual)`。
- admin 手动打款完成后调用 `complete-manual-payout`，必须填写 external tx id / proof 文本，并可上传图片凭证。
- `complete-manual-payout` 只能从 `pending` / `processing` / `needs_review` 的 manual payout 执行，ledger：
  ```text
  payout_reserved -> settlement_paid net_payout_usd
  payout_reserved -> fee_revenue    payout_fee_usd
  ```
- 如果 admin 拒绝或 provider 撤回，必须调用 payout cancel / mark-failed 释放：
  ```text
  payout_reserved -> client_payable amount_usd
  ```
- admin 所谓“修改 provider 待提现余额”只能通过 ledger 实现：
  - 已经手动打款：用 `complete-manual-payout` 结算锁定余额。
  - 工单取消/失败：释放 `payout_reserved` 回 `client_payable`。
  - 额外补偿或扣减：用 `adjust-provider-payable` 创建 `manual_adjustment` ledger entry，并要求填写 reason、关联 ticket、admin actor。

`adjust-provider-payable` ledger：

```text
risk_loss -> client_payable        compensation_amount
client_payable -> risk_loss        clawback_amount
```

约束：

- `clawback_amount` 不能超过该 owner 当前 `client_payable` 可用余额。
- 所有人工调账必须出现在 provider `/claim` 收益明细和 admin `money-events` 中。
- 工单状态变化、admin 内部备注、人工打款凭证都写 `admin_audit` 或 ticket timeline。

## 16. Phase 13 - 风控、审计、Admin API

Admin 使用 `MARKET_ADMIN_EMAILS` 邮箱白名单；后端只根据已验证 Web session email 判定 admin。

Admin endpoints：

| Method | Path | 用途 |
|---|---|---|
| GET | `/v1/admin/users` | 用户列表 |
| GET | `/v1/admin/users/{id}` | 用户详情 |
| GET | `/v1/admin/users/{id}/ledger` | 用户流水 |
| POST | `/v1/admin/users/{id}/adjust` | 人工调账 |
| GET | `/v1/admin/topups` | 充值订单 |
| GET | `/v1/admin/topups/{id}` | 充值详情 |
| POST | `/v1/admin/topups/{id}/refund` | 主动退款流程 |
| GET | `/v1/admin/webhooks/dodo` | webhook 事件 |
| GET | `/v1/admin/models` | 模型列表，包含状态、价格摘要、路由摘要 |
| POST | `/v1/admin/models` | 新增模型 |
| GET | `/v1/admin/models/{id}` | 模型详情 |
| PATCH | `/v1/admin/models/{id}` | 修改模型元信息、展示状态、排序 |
| POST | `/v1/admin/models/{id}/activate` | 上线模型 |
| POST | `/v1/admin/models/{id}/deactivate` | 下线模型 |
| PUT | `/v1/admin/models/{id}/price` | 修改模型统一 token 价格 |
| GET | `/v1/admin/models/{id}/price-changes` | 单模型改价审计 |
| PUT | `/v1/admin/models/{id}/routing` | 设置模型路由模式 all / include_only / exclude |
| PUT | `/v1/admin/models/{id}/routing/shares` | 设置模型绑定 share |
| POST | `/v1/admin/models/route-preview` | 预览某模型当前会路由到哪些 share |
| GET | `/v1/admin/price-changes` | 改价审计 |
| GET | `/v1/admin/shares` | share 缓存 |
| GET | `/v1/admin/money/overview` | 资金工作台总览聚合，MVP 可由前端临时聚合现有接口 |
| GET | `/v1/admin/money/search` | 资金跨对象搜索，V2 实现 |
| GET | `/v1/admin/charges` | 资金 / API 计费：请求账单 |
| GET | `/v1/admin/earnings` | 资金 / Provider 收益：应付汇总 |
| GET | `/v1/admin/ledger` | 资金 / 账本：全局账本查询 |
| GET | `/v1/admin/money-events` | 资金 / 事件流：充值/消费/收入/提现/退款统一事件流 |
| GET | `/v1/admin/settlements` | 结算 |
| GET | `/v1/admin/payout-requests` | provider 提现请求 |
| POST | `/v1/admin/payout-requests/{id}/execute-gateio` | 触发 Gate.io 自动转账 |
| POST | `/v1/admin/payout-requests/{id}/mark-paid` | 标记打款完成 |
| POST | `/v1/admin/payout-requests/{id}/mark-failed` | 标记失败 |
| POST | `/v1/admin/payout-requests/{id}/cancel` | 取消未执行提现并释放锁定余额 |
| GET | `/v1/admin/tickets` | 工单列表 |
| GET | `/v1/admin/tickets/{id}` | 工单详情 |
| POST | `/v1/admin/tickets/{id}/assign` | 分配工单 |
| POST | `/v1/admin/tickets/{id}/messages` | 回复工单 / 内部备注 |
| POST | `/v1/admin/tickets/{id}/status` | 修改工单状态 |
| POST | `/v1/admin/tickets/{id}/link-payout` | 关联已有提现请求 |
| POST | `/v1/admin/tickets/{id}/complete-manual-payout` | 完成人工提现并结算锁定余额 |
| POST | `/v1/admin/tickets/{id}/adjust-provider-payable` | 工单关联 provider 应付余额调账 |
| GET | `/v1/admin/audit` | admin audit |

审计：

- admin writes 都写 `admin_audit`。
- 原始附件/导出/凭证写对象存储。
- API key last_used_at / last_used_ip_country 必须更新。
- 邮箱验证码发送/验证风控由 router 统一执行；market 对 session endpoint 和 API key 做 rate limit。

## 17. Phase 14 - Web 用户门户

MVP 不再拆多个独立 Web 页面，按当前实现采用“少量入口 + Tab 聚合”的页面契约，降低静态导出、router tunnel、登录回跳和前端状态同步复杂度。

Next.js 页面：

| 路径 | 内容 |
|---|---|
| `/` | 落地页；包含价格表、资金流说明、SDK 接入 curl 示例、登录 CTA |
| `/dashboard` | 用户控制台；Tab 聚合余额、充值、API key、usage、账本、账号状态 |
| `/claim` | Provider 收益明细、提现预览、Gate.io 提现、其他收款方式提现工单 |
| `/support` | 用户反馈工单、工单列表、工单详情、附件上传 |
| `/admin` | Admin 单页控制台；Tab 聚合用户、模型、shares、资金工作台、工单、公告、审计 |

不规划独立页面：

- 不新增 `/pricing`：价格展示保留在首页价格区块，并由 `GET /v1/prices` 驱动。
- 不新增 Web `/docs`：`GET /docs` 当前作为后端 JSON API/docs 入口；SDK 接入说明保留在首页，后续如需人类可读文档再新增 `/developer`，避免与后端 `/docs` 冲突。
- 不新增 `/login`：登录使用全局邮箱验证码弹窗/组件；需要登录时由页面触发弹窗，不做单独 route。
- 不新增 `/wallet`、`/wallet/topup/return`、`/api-keys`、`/usage`、`/settings`：这些能力收敛到 `/dashboard` tabs；Dodo 回跳后回到 `/dashboard` 并按 topup id 轮询或刷新充值状态。

Auth：

- market 负责 HttpOnly session cookie 与 CSRF 防护。
- 前端不读 router token，不读 JWT。
- 后端只信 market session store 中的已验证 email。
- 未登录访问需要鉴权的 tab 或操作时，前端显示登录弹窗；登录成功后刷新当前页面数据，不依赖独立 `/login` 跳转。

### 17.1 全局导航工具区

所有 Web 页面共用顶部导航，右侧工具区包含：

- **系统公告**：导航栏仅显示一个铃铛按钮，不显示“系统公告”文字；点击后打开公告弹窗。公告内容使用站点静态配置驱动，展示最近更新、价格提示、维护通知；MVP 不引入后端公告 API，V2 再扩展为 admin/remote config 可运营公告。
- **语言切换**：默认 `zh-CN`，支持轻量 `EN` 切换；MVP 使用前端本地字典，不引入 next-intl。
- **登录状态**：未登录显示邮箱登录按钮，已登录显示邮箱与退出；admin 链接只对 `isAdmin=true` 的 session 显示。

约束：

- 公告与语言都属于全局 UI 状态，应挂在 layout/provider 层，而不是分散在具体页面中。
- 移动端不单独做二级页面，以上工具项统一收纳到导航抽屉顶部。
- 当前网站默认保持明亮主题，不再提供用户侧明暗切换入口。
- language 的切换不能影响现有静态导出与 rust-embed 部署方式。

### 17.2 Admin 公告入口

- 公告后台化不进入当前主线。
- MVP 可继续使用前端静态公告文件；如已有 `/admin` 公告预览入口，可保留为过渡工具，但不作为阻塞项或验收项。
- V2 如需真正在线编辑，再增加后端公告表、admin 保存接口与发布状态。

### 17.3 去阴影 UI 迁移原则

本段属于设计规范，不再作为主计划里程碑。后续应拆到单独的 Web / UI guideline 文档；是否去阴影不影响当前交易链路与资金闭环交付。

`/claim` 行为：

- 未登录时显示登录弹窗。
- 登录 email 没有 provider 收益时显示空态和说明：需要在 cc-switch 中选择该 market 并开启 `ForSale=Yes`。
- `availableUsd < 1.00` 时只展示待提现余额，不显示提交按钮。
- `availableUsd >= 1.00` 时允许选择 Gate.io 自动提现，或选择“其他收款方式”填写文字/图片创建提现工单。

`/dashboard` Tab 契约：

- `overview`：余额、近 7 天消费、API key 数、近期事件摘要。
- `topup`：Dodo 一次性充值、充值订单状态、充值手续费说明。
- `api-keys`：创建、重命名、撤销 API key，展示 prefix、created_at、last_used_at、last_used_ip_country。
- `usage`：请求账单、token usage、单价快照、request id、状态筛选。
- `ledger`：账本；用户可见资金流水，展示 gross / fee / net、reference、status。中文 UI 显示“账本”。
- `account`：当前登录邮箱、session 状态、退出登录。

`/support` 行为：

- API 用户可提交使用反馈、bug、账单问题，并附带文字和图片。
- Provider 可提交收益/提现问题；如果是提现，优先引导到 `/claim` 创建提现工单，保证余额先锁定。
- 工单详情展示消息时间线、附件、关联 request / payout / ledger references。

## 18. Phase 15 - Admin UI

Admin UI 不拆独立子路由，统一使用 `/admin` 单页 Tab 控制台。顶层 Tab 以运营任务归类；和钱相关的能力统一收敛到“资金”工作台，避免 admin 在“计费 / 收益 / Money 事件 / Ledger”之间理解底层边界。

- `overview`：运营概览、待处理提现、待处理工单、账本一致性检查。
- `users`：用户列表、用户详情、用户流水、人工调账。
- `models`：模型增删改、上下线、统一 token 价格、改价审计、share 路由规则和路由预览；首页展示由状态自动决定。
- `shares`：router share 缓存、手动同步、健康状态。
- `money`：资金工作台，聚合充值、API 计费、Provider 收益、提现、资金事件流、账本和对账检查。
- `tickets`：工单列表、工单详情、回复、内部备注、人工提现完成、provider payable 调账。
- `announcements`：静态公告预览、复制 JSON、核对中英文内容，作为过渡期公告运营入口。
- `audit`：admin audit。

资金工作台内部子 Tab：

- `overview`：资金总览卡片，包括用户余额、provider 应付、预授权中、待提现、手续费收入、risk_loss、账本一致性状态。
- `events`：资金事件流，即原 `money-events`，按业务时间线展示充值、消费、收入、提现、退款、手续费、人工调账。
- `charges`：API 计费，即原 `charges`，展示 `request_charges`、usage、价格快照、needs_review 手动结算/释放。
- `earnings`：Provider 收益，即原 `earnings`，按 owner email 汇总 `client_payable`。
- `topups`：充值，即原 `topups`，展示 Dodo 订单、webhook、主动退款。
- `payouts`：提现，即原 `payouts`，展示 payout request、Gate.io 执行、mark-paid、mark-failed、cancel。
- `ledger`：账本，即原 `ledger`，展示最底层资金移动明细和一致性校验；中文 UI 显示“账本”，不显示英文 Ledger。
- `check`：对账检查，展示账本一致性检查、reserved drift、payout_reserved drift 和异常项。

资金工作台规则：

- UI 合并，事实源不合并；`request_charges`、`ledger_entries`、`topup_orders`、`payout_requests` 等仍保持独立表。
- 默认进入 `money/events` 或 `money/overview`，再通过全局筛选定位 request id、topup id、payout id、provider email、用户 email。
- 点击任意资金事件打开详情抽屉，展示关联对象：request charge、账本 entries、topup order、payout request、object proof、webhook raw payload、admin audit。
- 顶层 legacy hash 保持兼容：`#topups`、`#charges`、`#earnings`、`#payouts`、`#ledger`、`#money` 都应跳转或映射到 `#money` 下对应子 Tab。

非 admin email session 不能访问，前端 redirect，后端二次拒绝。

Admin UI 提现交互：

- TanStack Query 管理 payout request 列表、`execute-gateio`、`mark-paid`、`mark-failed`、`cancel` 的 loading/success/error 状态。
- Zod 校验批量转账列表：Gate.io UID/email 至少一项、金额大于 0、`netPayoutUsd = amountUsd - feeUsd`。
- 执行 Gate.io 前显示二次确认，列出总笔数、总 gross、总 fee、总 net、目标脱敏值。

Admin UI 工单交互：

- 工单列表支持按 type/status/priority/assignee/owner_email/user_id 过滤。
- `payout_manual` 工单详情必须展示锁定金额、手续费、net payout、provider 收款说明、附件、账本状态。
- `complete-manual-payout` 前必须二次确认，并要求填写 external tx id 或 proof 文本；可上传打款截图。

## 19. cc-switch 配套改造（Phase A）

cc-switch 在用户选择 `ForSale=Yes` 和 market 后：

1. 调 router `GET /v1/markets`。
2. 以 router 返回的 `display_name`、`email`、`public_base_url` 作为 market 选择事实源；`GET /v1/public/info` 仅作为进入 market 后的增强信息，可懒加载，不作为前置强依赖。
3. 将 market email 加入 `shared_with_emails`。
4. 同步 share 到 router。
5. 显示 “Claim earnings on market” 链接，跳转 market `/claim`。

cc-switch 不接 market Web auth，不填写 Gate.io email / uid，不调用 market provider 收益接口。provider 收益和提现统一在 market Web 通过 router 邮箱验证码登录后处理。market 统一 token 价格展示保留在 market Web，不作为 client MVP 阻塞项。

## 20. 运维与测试

E2E 环境：

```text
cc-switch-router
cc-switch-market
mock cc-switch client backend
mock Dodo webhook
Turso dev database / local SQLite
local object store fixture / V2 R2 或 MinIO S3-compatible mock
router Resend email auth test config
```

E2E 用例：

1. 用户在 market Web 全局登录弹窗输入邮箱并完成 router Resend 验证码登录。
2. market 创建 HttpOnly session 并 upsert user。
3. Dodo top-up webhook 入账。
4. 创建 API key。
5. mock client 创建 `ForSale=Yes` share 并授权 market。
6. `cc-switch-market login` 完成 router 邮箱验证码登录。
7. `cc-switch-market account` 可显示登录 email、router session 状态、market public URL。
8. market 启动时自动调用 `/v1/markets/register` 注册 / 续期 market registry。
9. market refresh router session 后 sync shares。
10. market 运行中执行 `cc-switch-market logout` 会被 lock 拒绝，并提示先停止进程。
11. market 停止后执行 `cc-switch-market logout` 可删除本地 session。
12. 重新 login 后 market 可再次启动并注册 / 续期。
13. market 通过 `/v1/markets/tunnel/lease` 建立自己的 router subdomain HTTP 入口。
14. `/v1/messages` 通过 market subdomain 跑通 router market proxy。
15. 校验 usage、ledger、request_charges、client_payable。
16. idempotency 重放不重复扣费。
17. Dodo refund 不影响 client_payable。
18. provider 用 owner email 登录 `/claim`，余额 ≥ 1 USD 后提交 Gate.io payout request。
19. admin 或后台任务触发 Gate.io batch transfer，成功后 ledger 从 `payout_reserved` 拆分到 `settlement_paid` 与 `fee_revenue`。
20. provider 选择其他收款方式创建提现工单，上传图片附件，admin 手动打款后通过工单完成 ledger 结算。
21. 普通用户提交反馈工单，admin 回复并关闭。
22. 对象存储中存在 raw webhook、Gate.io request/response、工单附件、settlement proof。

SSE 运维：

- nginx：`proxy_buffering off; proxy_cache off; proxy_set_header X-Accel-Buffering no;`
- Cloudflare 或其他边缘代理必须确认 event-stream 不被聚合缓冲。

## 21. 里程碑顺序

1. P0：主链路修复
   - router market proxy + registry
   - client `app_type` 与 market 选路对齐
   - router share 自动同步
   - `parallel_limit = -1` 支持
   - `/_market/proxy` 双向流式透传
2. P1：market 基础设施、schema + ledger、Router Resend Web auth、API key、模型/价格/路由运营、用户门户
3. P2：proxy、预授权、扣费、分润、Dodo top-up、provider claim、Gate.io payout、support / admin 工作台
4. P3：cc-switch Phase A 产品闭环
5. 后置增强：usage 共享库、OpenAPI、公告后台化、普通请求全量对象留档、client 价格展示增强

## 22. MVP 验收

1. 用户可通过 router Resend 邮箱验证码登录 market Web。
2. 后端可通过 market session cookie 解析已验证 email，并拒绝伪造 header。
3. libSQL ledger balance check 通过。
4. 对象存储至少可写 raw webhook、Gate.io proof、工单附件等关键证据对象。
5. Dodo top-up 入账。
6. API user 创建 `sk-cs-` key。
7. cc-switch share 授权 market。
8. `cc-switch-market login` 已完成 router 邮箱验证码登录，market 可 refresh router session。
9. `cc-switch-market account` 可显示当前 router 登录 email 和 market public URL。
10. `cc-switch-market logout` 在 market server 运行中会拒绝执行，停止后可删除 session。
11. market 启动时自动注册 / 续期 router market registry，email 来自 router session 而不是 env。
12. market HTTP 入口可通过 `https://{market_subdomain}.{router_domain}` 访问。
13. market 选路并通过 router market proxy 调用 client；`parallel_limit = -1` 的 share 可被正确选中。
14. 成功请求完成 usage 解析、扣用户余额、记 client payable。
15. 同 `Idempotency-Key` 不重复扣费。
16. refund 不回滚 client payable。
17. provider 可用 owner email 登录 `/claim` 查看待提现余额。
18. 待提现余额大于 1 USD 时可提交 Gate.io uid/email payout request。
19. provider 可选择其他收款方式创建提现工单，文字和图片附件进入对象存储。
20. admin 可处理 payout request，触发 Gate.io 自动转账并查看 Gate.io response / proof。
21. admin 手动打款后可通过提现工单完成 `payout_reserved -> settlement_paid / fee_revenue` ledger 结算。
22. 用户可提交普通反馈工单，admin 可回复和关闭。
23. client 可通过 router `GET /v1/markets` 选择 market，自动写入 `shared_with_emails`，并跳转 market `/claim`。

## 23. 参考

- Router Resend 邮箱验证码接口：`/data/projects/cc-switch-router/src/api.rs` 与 `/data/projects/cc-switch-router/src/store.rs`
- Turso / libSQL / SQLite docs
- Cloudflare R2 S3-compatible API docs（V2 对象存储后端参考）
- Gate.io API V4 official docs: Batch Transfers、HMAC-SHA512 auth、official Rust SDK auth example
