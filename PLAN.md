# 分层调度系统规划与完成情况

## 架构总览("Plan B")

调度职责按**数据归属**切分到两个服务:

- **cc-switch-router(路由端)**:拥有实时并发真相(inflight-by-share、在线分钟、配额窗口),负责计算每个 share 的调度信号 —— `quota_health` / `stability` / `headroom` / `owner_penalty`,通过 `/v1/market/shares` 下发。
- **cc-switch-market(市场端)**:落库这些信号,在请求时按调度策略(profile)对应的权重做 `base_score` 排序选 share。

429/rate_limited 反馈由市场端回传路由端(`/v1/market/shares/feedback`),路由端据此对**同一 owner**(共享同一上游凭据)施加临时惩罚。

## Sprint 进度

| Sprint | 内容 | 状态 |
| --- | --- | --- |
| S1 | 路由端信号计算 + OverrideStore + 反馈/headroom 端点 | ✅ 完成 |
| S2 | 市场端 `router_shares` 信号列 + RouterShare 反序列化 + 同步落库 | ✅ 完成 |
| S3 | `select_share_candidates` 引入 base_score 排序 + 429 反馈回传 | ✅ 完成 |
| S4 | SchedulingProfile 模块 + 参数化权重 + 管理后台 UI | ✅ 完成 |

后端 33 个单测全部通过;前端 `npm run typecheck` 通过。

### S1 — 路由端(cc-switch-router)
- 新建 `src/scheduling_signals.rs`:`compute_quota_health/stability/headroom` + `OverrideStore` + 请求/响应类型 + 12 个测试。
  - 关键常量:`QUOTA_WINDOW_MIN_TTL_S=3600`、`QUOTA_SOFTMIN_ALPHA=10.0`、`QUOTA_URGENCY_HORIZON_S=18000`、`STABILITY_W10_MAX=0.7`、`HEADROOM_FLOOR=0.1`、`OVERRIDE_DEFAULT_TTL=30min`。
- `models.rs`:新增 `ShareSignals`,`MarketShareView` 增加 `signals` + `share_created_at`。
- `main.rs`:`ServerState` 增加 `scheduling_overrides: Arc<OverrideStore>`,接入 cleanup_task 做过期清理。
- `store.rs`:`list_market_shares` SQL 增加 `created_at`,新增 `list_online_minutes_10m` / `share_parallel_limits`(批量 IN)/`lookup_share_owner_email`,逐行内联计算信号。
- `api.rs`:新增 `POST /v1/market/shares/headroom`(256 id 上限,实时 inflight + parallel_limit)与 `POST /v1/market/shares/feedback`(默认惩罚 0.5 / TTL 30m,硬上限 24h)。`market_shares` handler 叠加 owner_penalty。

### S2 — 市场端信号落库(cc-switch-market)
- `db.rs`:6 条 `ALTER TABLE router_shares`(`quota_health` REAL DEFAULT 0.5、`stability` REAL DEFAULT 1.0、`headroom` REAL DEFAULT 1.0、`samples_10m` INTEGER DEFAULT 0、`owner_penalty` REAL DEFAULT 1.0、`share_created_at` TEXT),新增 `impl IntoDbValue for f64`。默认值取宽松值,避免新迁移行在首次同步前被惩罚。
- `router_client.rs`:新增 `ShareSignals`(camelCase serde + 宽松 Default),`RouterShare` 增加 `signals` + `share_created_at`,UPSERT 写入 6 个新列。对旧版路由端向后兼容(信号缺失则用默认值)。

### S3 — base_score 排序 + 反馈(cc-switch-market)
- `proxy.rs`:`ORDER BY` 改为参数化 base_score:

  ```sql
  ORDER BY (
    (?15 * stability + ?16 * quota_health
     + ?17 * CASE WHEN parallel_limit = -1 THEN 1.0
                  ELSE 1.0 - (CAST(active_requests AS REAL) / CAST(parallel_limit AS REAL))
             END
     + ?18 * 1.0)
    * (1.0 + ?19 * 0.5)
    * owner_penalty
    - 0.05 * MIN(failure_count, 5)
  ) DESC, priority DESC, COALESCE(last_success_at, last_seen_at) DESC
  ```
- `select_share_candidates` 入口解析 profile 并取权重。
- 新增 `maybe_report_router_feedback`:仅在 `kind == "rate_limited"` 时 fire-and-forget 回传路由端,不阻塞请求热路径。

### S4 — 策略模块 + 管理 UI(cc-switch-market)
- 新建 `src/scheduling.rs`:`SchedulingProfile` 枚举(7 变体)+ `ProfileWeights` + `from_kebab`(兼容 kebab/snake)+ `resolve_profile`(读 `scope_json["schedulingProfile"]` 或 `scheduling_profile`)+ 8 个测试。
- `main.rs`:注册 `mod scheduling;`。
- 前端 `web/app/dashboard/ui.tsx`:
  - 编辑/新建 API 密钥弹窗加入策略 `<select>`(7 选项)。
  - `ApiKeyLimitFormValue` 增加 `schedulingProfile`;`apiKeySchedulingProfile` 读 scope(兼容 camel/snake);`buildLimitPayload` 与 `agent_model_vendors` 一起写回。
  - 密钥列表行展示当前策略。
- `web/lib/copy.ts`:中英文补 `schedulingLabel`/`schedulingHint`/`schedulingProfiles`/`limitScheduling`。

> 后端将 `scope_json` 原样落库,故 camelCase 的 `schedulingProfile` 能原封不动到达 `resolve_profile`。

## 调度策略与权重

权重作用于已在 `[0,1]` 区间的信号,信号权重之和约为 1.0;`price_bias` 是对最终 base_score 的独立乘子(>0 偏好便宜 share,<0 偏好昂贵 share)。

| Profile | stability | quota_health | headroom | freshness | price_bias |
| --- | --- | --- | --- | --- | --- |
| `balanced`(均衡,默认) | 0.35 | 0.30 | 0.25 | 0.10 | 0.00 |
| `price-first`(价格优先) | 0.25 | 0.40 | 0.25 | 0.10 | +0.50 |
| `stability-first`(稳定优先) | 0.55 | 0.20 | 0.20 | 0.05 | 0.00 |
| `fresh-quota`(新鲜额度) | 0.20 | 0.55 | 0.15 | 0.10 | 0.00 |
| `diversify`(分散负载) | 0.25 | 0.20 | 0.30 | 0.25 | 0.00 |
| `premium`(高质量) | 0.45 | 0.20 | 0.30 | 0.05 | -0.10 |
| `budget-aware`(预算感知) | 0.25 | 0.35 | 0.25 | 0.15 | +0.40 |

无法解析或缺省时回落到 `balanced`。

## 不可覆盖的硬性护栏

无论 profile 如何配置,以下底线始终生效、不可被策略覆盖:

- 资金 / 并发底线
- cooldown 下限
- 可重试黑名单(`auth_failed` 永不可重新加入为可重试)
- `monthly_spend_cap`
- `min_request_balance` 下限

## 延后事项(暂未排期)

- 用户级策略叠加(需要 `users.scope_json` 列;目前仅 api-key 级解析)
- `per_model_overrides` 表(按模型覆盖)
- PROBE 分散算法(deterministic hash(request_id+share_id)、top-K 末位替换、连败保护)
- base_score 效果埋点 / 遥测

## 待人工验证

- 尚未启动 `next dev` 在浏览器中点开新策略选择器,仅确认了可编译 + typecheck 通过,未验证实际渲染与保存。
