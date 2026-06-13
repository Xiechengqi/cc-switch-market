use axum::{
    Json,
    extract::{Path, Query, State},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{app_state::AppState, auth::AdminPrincipal, error::ApiError};

#[derive(Deserialize)]
pub struct PriceChangesQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PriceItem {
    pub id: Uuid,
    pub model_id: Option<Uuid>,
    pub app_type: String,
    pub model_pattern: String,
    pub display_name: Option<String>,
    pub is_public: Option<bool>,
    pub input_per_million: Decimal,
    pub output_per_million: Decimal,
    pub cache_read_per_million: Option<Decimal>,
    pub cache_write_per_million: Option<Decimal>,
    pub official_input_per_million: Option<Decimal>,
    pub official_output_per_million: Option<Decimal>,
    pub official_cache_read_per_million: Option<Decimal>,
    pub official_cache_write_per_million: Option<Decimal>,
    pub discount_percent: Decimal,
    pub currency: String,
    pub status: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ModelItem {
    pub id: Uuid,
    pub app_type: String,
    pub model_pattern: String,
    pub display_name: Option<String>,
    pub status: String,
    pub is_public: bool,
    pub sort_order: i64,
    pub price: Option<PriceItem>,
    pub routing: Option<RoutingRuleItem>,
    pub routeable_shares: i64,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RoutingRuleItem {
    pub id: Uuid,
    pub model_id: Uuid,
    pub mode: String,
    pub priority: i64,
    pub enabled: bool,
    pub notes: Option<String>,
    pub shares: Vec<RoutingShareItem>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RoutingShareItem {
    pub router_id: String,
    pub share_id: String,
}

#[derive(Deserialize)]
pub struct UpsertPrice {
    pub app_type: String,
    pub model_pattern: String,
    pub input_per_million: Decimal,
    pub output_per_million: Decimal,
    pub cache_read_per_million: Option<Decimal>,
    pub cache_write_per_million: Option<Decimal>,
    pub status: Option<String>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateModel {
    pub app_type: String,
    pub model_pattern: String,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub sort_order: Option<i64>,
    pub input_per_million: Option<Decimal>,
    pub output_per_million: Option<Decimal>,
    pub cache_read_per_million: Option<Decimal>,
    pub cache_write_per_million: Option<Decimal>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct PatchModel {
    pub app_type: Option<String>,
    pub model_pattern: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Deserialize)]
pub struct ModelPriceInput {
    pub input_per_million: Decimal,
    pub output_per_million: Decimal,
    pub cache_read_per_million: Option<Decimal>,
    pub cache_write_per_million: Option<Decimal>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct VendorDiscountInput {
    pub discount_percent: Decimal,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct VendorDiscountItem {
    pub app_type: String,
    pub discount_percent: Decimal,
}

#[derive(Deserialize)]
pub struct RoutingInput {
    pub mode: String,
    pub priority: Option<i64>,
    pub enabled: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct RoutingSharesInput {
    pub shares: Vec<RoutingShareItem>,
}

#[derive(Deserialize)]
pub struct RoutePreviewInput {
    pub app_type: String,
    pub model: String,
}

pub async fn list_prices(State(state): State<AppState>) -> Result<Json<Vec<PriceItem>>, ApiError> {
    Ok(Json(fetch_prices(state.db(), true).await?))
}

pub async fn admin_list_prices(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<Vec<PriceItem>>, ApiError> {
    Ok(Json(fetch_prices(state.db(), false).await?))
}

pub async fn upsert_price(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<UpsertPrice>,
) -> Result<Json<PriceItem>, ApiError> {
    let db = state.db();
    let old = db
        .query_optional(
            "SELECT id, app_type, model_pattern, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million, currency, status FROM model_prices WHERE id = ?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let now = crate::db::now_string();
    let app_type = input.app_type;
    let model_pattern = input.model_pattern;
    let input_per_million = input.input_per_million;
    let output_per_million = input.output_per_million;
    let cache_read_per_million = input.cache_read_per_million.unwrap_or(Decimal::ZERO);
    let cache_write_per_million = input.cache_write_per_million.unwrap_or(Decimal::ZERO);
    let status = input.status.unwrap_or_else(|| "active".to_string());
    let model_id = ensure_model(db, &app_type, &model_pattern, None).await?;
    if input_per_million < Decimal::ZERO
        || output_per_million < Decimal::ZERO
        || cache_read_per_million < Decimal::ZERO
        || cache_write_per_million < Decimal::ZERO
    {
        return Err(ApiError::bad_request(
            "invalid_price",
            "model prices must be zero or positive",
        ));
    }
    if status == "active" {
        let duplicate = db
            .query_optional(
                "SELECT id FROM model_prices WHERE model_id=?1 AND status='active' AND id <> ?2 LIMIT 1",
                vec![
                    crate::db::uuid_val(model_id),
                    crate::db::uuid_val(id),
                ],
            )
            .await?;
        if duplicate.is_some() {
            return Err(ApiError::conflict(
                "duplicate_active_price",
                "an active price already exists for this app_type/model_pattern",
            ));
        }
    }
    db.execute(
        r#"
        INSERT INTO model_prices
          (id, model_id, app_type, model_pattern, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million, status, effective_from, created_at, updated_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?10)
        ON CONFLICT (id) DO UPDATE SET
          model_id = excluded.model_id,
          app_type = excluded.app_type,
          model_pattern = excluded.model_pattern,
          input_per_million = excluded.input_per_million,
          output_per_million = excluded.output_per_million,
          cache_read_per_million = excluded.cache_read_per_million,
          cache_write_per_million = excluded.cache_write_per_million,
          status = excluded.status,
          updated_at = excluded.updated_at
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::uuid_val(model_id),
            crate::db::val(&app_type),
            crate::db::val(&model_pattern),
            crate::db::dec_val(input_per_million),
            crate::db::dec_val(output_per_million),
            crate::db::dec_val(cache_read_per_million),
            crate::db::dec_val(cache_write_per_million),
            crate::db::val(&status),
            crate::db::val(&now),
        ],
    )
    .await?;
    let row = db
        .query_one(
            r#"
            SELECT mp.id, mp.model_id, mp.app_type, mp.model_pattern, m.display_name, m.is_public,
                   mp.input_per_million, mp.output_per_million, mp.cache_read_per_million,
                   mp.cache_write_per_million, mp.currency, mp.status, COALESCE(vd.discount_percent, '10') AS discount_percent
              FROM model_prices mp
              LEFT JOIN models m ON m.id = mp.model_id
              LEFT JOIN model_vendor_discounts vd ON vd.app_type = mp.app_type
             WHERE mp.id = ?1
            "#,
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let new_snapshot = serde_json::json!({
        "id": row.string("id"),
        "app_type": row.string("app_type"),
        "model_pattern": row.string("model_pattern"),
    });
    db.execute(
        "INSERT INTO price_changes (id, price_id, old_snapshot, new_snapshot, admin_actor, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(id),
            old.map(price_snapshot_val).unwrap_or(libsql::Value::Null),
            crate::db::json_val(new_snapshot),
            crate::db::val(&admin.email),
            crate::db::opt_val(input.reason.clone()),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    db.execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,'price.upsert','price',?3,?4,?5)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(&admin.email),
            crate::db::uuid_val(id),
            crate::db::json_val(serde_json::json!({"reason": input.reason})),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;

    Ok(Json(row_to_price(row)))
}

pub async fn admin_list_models(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<Vec<ModelItem>>, ApiError> {
    Ok(Json(fetch_models(state.db()).await?))
}

pub async fn admin_put_model_vendor_discount(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(app_type): Path<String>,
    Json(input): Json<VendorDiscountInput>,
) -> Result<Json<VendorDiscountItem>, ApiError> {
    validate_app_type(&app_type)?;
    if input.discount_percent <= Decimal::ZERO || input.discount_percent > Decimal::from(100u32) {
        return Err(ApiError::bad_request(
            "invalid_discount_percent",
            "discount percent must be greater than 0 and no more than 100",
        ));
    }
    let now = crate::db::now_string();
    state
        .db()
        .execute(
            r#"
            INSERT INTO model_vendor_discounts (app_type, discount_percent, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(app_type) DO UPDATE SET
              discount_percent=excluded.discount_percent,
              updated_at=excluded.updated_at
            "#,
            vec![
                crate::db::val(&app_type),
                crate::db::dec_val(input.discount_percent),
                crate::db::val(&now),
            ],
        )
        .await?;
    write_admin_audit(
        &state,
        &admin.email,
        "model_vendor_discount.update",
        "model_vendor_discount",
        Uuid::new_v4(),
        serde_json::json!({
            "app_type": app_type,
            "discount_percent": input.discount_percent.to_string(),
        }),
    )
    .await?;
    Ok(Json(VendorDiscountItem {
        app_type,
        discount_percent: input.discount_percent,
    }))
}

pub async fn admin_get_model(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<ModelItem>, ApiError> {
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_create_model(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Json(input): Json<CreateModel>,
) -> Result<Json<ModelItem>, ApiError> {
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    let status = input.status.unwrap_or_else(|| "active".to_string());
    validate_app_type(&input.app_type)?;
    validate_status(&status)?;
    ensure_vendor_discount(state.db(), &input.app_type).await?;
    state.db().execute(
        r#"
        INSERT INTO models
          (id, app_type, canonical_name, model_pattern, display_name, status, is_public, sort_order, aliases_json, metadata_json, created_at, updated_at)
        VALUES (?1,?2,?3,?3,?4,?5,?6,?7,'[]','{}',?8,?8)
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::val(&input.app_type),
            crate::db::val(&input.model_pattern),
            crate::db::opt_val(input.display_name.clone()),
            crate::db::val(&status),
            crate::db::val(input.model_pattern != "*"),
            crate::db::val(input.sort_order.unwrap_or(0)),
            crate::db::val(&now),
        ],
    ).await?;
    if let (Some(input_price), Some(output_price)) =
        (input.input_per_million, input.output_per_million)
    {
        upsert_model_price_inner(
            &state,
            &admin.email,
            id,
            ModelPriceInput {
                input_per_million: input_price,
                output_per_million: output_price,
                cache_read_per_million: input.cache_read_per_million,
                cache_write_per_million: input.cache_write_per_million,
                reason: input.reason,
            },
        )
        .await?;
    }
    write_admin_audit(
        &state,
        &admin.email,
        "model.create",
        "model",
        id,
        serde_json::json!({}),
    )
    .await?;
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_patch_model(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<PatchModel>,
) -> Result<Json<ModelItem>, ApiError> {
    let current = model_row(state.db(), id).await?;
    let app_type = input.app_type.unwrap_or_else(|| current.string("app_type"));
    let model_pattern = input
        .model_pattern
        .unwrap_or_else(|| current.string("model_pattern"));
    let status = input.status.unwrap_or_else(|| current.string("status"));
    validate_app_type(&app_type)?;
    validate_status(&status)?;
    state
        .db()
        .execute(
            r#"
        UPDATE models
           SET app_type=?2, canonical_name=?3, model_pattern=?3, display_name=?4,
               status=?5, is_public=?6, sort_order=?7, updated_at=?8
         WHERE id=?1
        "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::val(&app_type),
                crate::db::val(&model_pattern),
                crate::db::opt_val(
                    input
                        .display_name
                        .or_else(|| current.opt_string("display_name")),
                ),
                crate::db::val(&status),
                crate::db::val(model_pattern != "*"),
                crate::db::val(
                    input
                        .sort_order
                        .unwrap_or_else(|| current.i64("sort_order")),
                ),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    state.db().execute(
        "UPDATE model_prices SET app_type=?2, model_pattern=?3, updated_at=?4 WHERE model_id=?1",
        vec![
            crate::db::uuid_val(id),
            crate::db::val(&app_type),
            crate::db::val(&model_pattern),
            crate::db::val(crate::db::now_string()),
        ],
    ).await?;
    write_admin_audit(
        &state,
        &admin.email,
        "model.update",
        "model",
        id,
        serde_json::json!({}),
    )
    .await?;
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_delete_model(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let model = model_row(state.db(), id).await?;
    if model.string("status") == "active" {
        return Err(ApiError::bad_request(
            "active_model_not_deletable",
            "active model must be deactivated before delete",
        ));
    }
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "DELETE FROM model_routing_rule_shares WHERE rule_id IN (SELECT id FROM model_routing_rules WHERE model_id=?1)",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM model_routing_rules WHERE model_id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM model_share_blocks WHERE model_id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM model_prices WHERE model_id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM models WHERE id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,'model.delete','model',?3,?4,?5)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(&admin.email),
            crate::db::uuid_val(id),
            crate::db::json_val(serde_json::json!({
                "app_type": model.string("app_type"),
                "model_pattern": model.string("model_pattern")
            })),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "deleted": true, "id": id })))
}

pub async fn admin_activate_model(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<ModelItem>, ApiError> {
    set_model_status(&state, &admin.email, id, "active").await
}

pub async fn admin_deactivate_model(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<ModelItem>, ApiError> {
    set_model_status(&state, &admin.email, id, "inactive").await
}

pub async fn admin_put_model_price(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<ModelPriceInput>,
) -> Result<Json<ModelItem>, ApiError> {
    upsert_model_price_inner(&state, &admin.email, id, input).await?;
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_model_price_changes(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
    Query(query): Query<PriceChangesQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let price = active_price_row_for_model(state.db(), id).await?;
    price_changes_for_sql(state.db(), Some(price.string("id")), query)
        .await
        .map(Json)
}

pub async fn admin_put_model_routing(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<RoutingInput>,
) -> Result<Json<ModelItem>, ApiError> {
    validate_routing_mode(&input.mode)?;
    model_row(state.db(), id).await?;
    let now = crate::db::now_string();
    let rule_id = existing_rule_id(state.db(), id)
        .await?
        .unwrap_or_else(Uuid::new_v4);
    state.db().execute(
        r#"
        INSERT INTO model_routing_rules (id, model_id, mode, priority, enabled, notes, created_at, updated_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
        ON CONFLICT(model_id) DO UPDATE SET
          mode=excluded.mode, priority=excluded.priority, enabled=excluded.enabled, notes=excluded.notes, updated_at=excluded.updated_at
        "#,
        vec![
            crate::db::uuid_val(rule_id),
            crate::db::uuid_val(id),
            crate::db::val(&input.mode),
            crate::db::val(input.priority.unwrap_or(0)),
            crate::db::val(input.enabled.unwrap_or(true)),
            crate::db::opt_val(input.notes),
            crate::db::val(now),
        ],
    ).await?;
    write_admin_audit(
        &state,
        &admin.email,
        "model.routing",
        "model",
        id,
        serde_json::json!({"mode": input.mode}),
    )
    .await?;
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_put_model_routing_shares(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<RoutingSharesInput>,
) -> Result<Json<ModelItem>, ApiError> {
    model_row(state.db(), id).await?;
    let rule_id = ensure_rule(state.db(), id).await?;
    state
        .db()
        .execute(
            "DELETE FROM model_routing_rule_shares WHERE rule_id=?1",
            vec![crate::db::uuid_val(rule_id)],
        )
        .await?;
    let now = crate::db::now_string();
    for share in input.shares {
        state.db().execute(
            "INSERT OR IGNORE INTO model_routing_rule_shares (rule_id, router_id, share_id, created_at) VALUES (?1,?2,?3,?4)",
            vec![
                crate::db::uuid_val(rule_id),
                crate::db::val(share.router_id),
                crate::db::val(share.share_id),
                crate::db::val(&now),
            ],
        ).await?;
    }
    write_admin_audit(
        &state,
        &admin.email,
        "model.routing_shares",
        "model",
        id,
        serde_json::json!({}),
    )
    .await?;
    fetch_model(state.db(), id).await.map(Json)
}

pub async fn admin_route_preview(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Json(input): Json<RoutePreviewInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let price = match_price(state.db(), &input.app_type, &input.model).await?;
    let model_id = price
        .model_id
        .ok_or_else(|| ApiError::bad_request("model_not_supported", "model is not supported"))?;
    let diagnostics = route_diagnostics(state.db(), model_id, &input.app_type).await?;
    Ok(Json(serde_json::json!({
        "model_id": model_id,
        "app_type": input.app_type,
        "model": input.model,
        "matched_pattern": price.model_pattern,
        "diagnostics": diagnostics,
    })))
}

pub async fn price_changes(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<PriceChangesQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    price_changes_for_sql(state.db(), None, query)
        .await
        .map(Json)
}

async fn price_changes_for_sql(
    db: &crate::db::Db,
    price_id: Option<String>,
    query: PriceChangesQuery,
) -> Result<crate::pagination::Page<serde_json::Value>, ApiError> {
    let mut sql =
        "SELECT id, price_id, old_snapshot, new_snapshot, admin_actor, reason, created_at FROM price_changes WHERE 1=1"
            .to_string();
    let mut params = vec![];
    if let Some(price_id) = price_id {
        sql.push_str(&format!(" AND price_id = ?{}", params.len() + 1));
        params.push(crate::db::val(price_id));
    }
    if let Some(cursor) = query.cursor.filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(" AND created_at < ?{}", params.len() + 1));
        params.push(crate::db::val(cursor));
    }
    sql.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ?{}",
        params.len() + 1
    ));
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = db.query_all(&sql, params).await?;
    let items = rows.into_iter().map(price_change_json).collect::<Vec<_>>();
    Ok(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| {
            item.get("created_at")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        },
    ))
}

pub async fn match_price(
    db: &crate::db::Db,
    app_type: &str,
    model: &str,
) -> Result<PriceItem, ApiError> {
    let prices = fetch_prices(db, false).await?;
    match_price_from_prices(prices, Some(app_type), model)
}

pub async fn match_concrete_price_any_app(
    db: &crate::db::Db,
    model: &str,
) -> Result<PriceItem, ApiError> {
    let prices = fetch_prices(db, false)
        .await?
        .into_iter()
        .filter(|price| price.model_pattern != "*")
        .collect();
    match_price_from_prices(prices, None, model)
}

fn match_price_from_prices(
    prices: Vec<PriceItem>,
    app_type: Option<&str>,
    model: &str,
) -> Result<PriceItem, ApiError> {
    let mut matching = prices
        .into_iter()
        .filter(|p| app_type.is_none_or(|app_type| p.app_type == app_type))
        .filter(|p| {
            p.model_pattern == "*"
                || p.model_pattern == model
                || p.model_pattern.ends_with('*')
                    && model.starts_with(p.model_pattern.trim_end_matches('*'))
        })
        .collect::<Vec<_>>();
    matching.sort_by_key(|p| {
        if p.model_pattern == "*" {
            0
        } else {
            p.model_pattern.len()
        }
    });
    let Some(price) = matching.pop() else {
        return Err(ApiError::bad_request(
            "model_not_supported",
            "model is not supported",
        ));
    };
    if price.status != "active" {
        return Err(ApiError::bad_request("model_offline", "model is offline"));
    }
    Ok(price)
}

pub fn cost(input_tokens: u64, output_tokens: u64, price: &PriceItem) -> Decimal {
    cost_with_cache(input_tokens, output_tokens, 0, 0, price)
}

pub fn cost_with_cache(
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    price: &PriceItem,
) -> Decimal {
    let million = Decimal::from(1_000_000u64);
    let billable_input_tokens = input_tokens.saturating_sub(cache_read_tokens);
    Decimal::from(billable_input_tokens) * price.input_per_million / million
        + Decimal::from(output_tokens) * price.output_per_million / million
        + Decimal::from(cache_read_tokens) * price.cache_read_per_million.unwrap_or(Decimal::ZERO)
            / million
        + Decimal::from(cache_write_tokens) * price.cache_write_per_million.unwrap_or(Decimal::ZERO)
            / million
}

pub async fn pricing_summary(db: &crate::db::Db) -> anyhow::Result<serde_json::Value> {
    let rows = db
        .query_all(
            r#"
            SELECT app_type, discount_percent
              FROM model_vendor_discounts
             WHERE app_type IN ('claude', 'codex', 'gemini', 'deepseek')
             ORDER BY app_type
            "#,
            vec![],
        )
        .await?;
    let mut summary = serde_json::Map::new();
    for row in rows {
        let app_type = row.string("app_type");
        let discount = row.decimal("discount_percent");
        summary.insert(app_type, serde_json::Value::String(discount.to_string()));
    }
    for app_type in ["claude", "codex", "gemini", "deepseek"] {
        summary
            .entry(app_type.to_string())
            .or_insert_with(|| serde_json::Value::Number(serde_json::Number::from(10)));
    }
    Ok(serde_json::Value::Object(summary))
}

async fn fetch_prices(db: &crate::db::Db, public_only: bool) -> Result<Vec<PriceItem>, ApiError> {
    let mut sql = r#"
        SELECT mp.id, mp.model_id, mp.app_type, mp.model_pattern, m.display_name, m.is_public, mp.input_per_million, mp.output_per_million, mp.cache_read_per_million,
               mp.cache_write_per_million, mp.currency, COALESCE(m.status, mp.status) AS status, COALESCE(vd.discount_percent, '10') AS discount_percent
          FROM model_prices mp
          LEFT JOIN models m ON m.id = mp.model_id
          LEFT JOIN model_vendor_discounts vd ON vd.app_type = mp.app_type
         WHERE 1=1
        "#.to_string();
    if public_only {
        sql.push_str(" AND mp.status='active' AND COALESCE(m.status, mp.status)='active' AND mp.model_pattern <> '*'");
    }
    sql.push_str(" ORDER BY mp.app_type, COALESCE(m.sort_order, 0), mp.model_pattern");
    let rows = db.query_all(&sql, vec![]).await?;
    Ok(rows.into_iter().map(row_to_price).collect())
}

fn row_to_price(row: crate::db::DbRow) -> PriceItem {
    let app_type = row.string("app_type");
    let model_pattern = row.string("model_pattern");
    let discount_percent = row.opt_decimal("discount_percent").unwrap_or(Decimal::TEN);
    let discount_multiplier = discount_percent / Decimal::from(100u32);
    let official_input_per_million = row.decimal("input_per_million");
    let official_output_per_million = row.decimal("output_per_million");
    let official_cache_read_per_million = row.decimal("cache_read_per_million");
    let official_cache_write_per_million = row.decimal("cache_write_per_million");
    let cache_read_missing = official_price_field_missing(&app_type, &model_pattern, "cache_read");
    let cache_write_missing =
        official_price_field_missing(&app_type, &model_pattern, "cache_write");
    PriceItem {
        id: row.uuid("id"),
        model_id: row.opt_uuid("model_id"),
        display_name: row.opt_string("display_name"),
        is_public: row.opt_string("is_public").map(|_| row.bool("is_public")),
        cache_read_per_million: if cache_read_missing {
            None
        } else {
            Some(official_cache_read_per_million * discount_multiplier)
        },
        cache_write_per_million: if cache_write_missing {
            None
        } else {
            Some(official_cache_write_per_million * discount_multiplier)
        },
        app_type,
        model_pattern,
        input_per_million: official_input_per_million * discount_multiplier,
        output_per_million: official_output_per_million * discount_multiplier,
        official_input_per_million: Some(official_input_per_million),
        official_output_per_million: Some(official_output_per_million),
        official_cache_read_per_million: if cache_read_missing {
            None
        } else {
            Some(official_cache_read_per_million)
        },
        official_cache_write_per_million: if cache_write_missing {
            None
        } else {
            Some(official_cache_write_per_million)
        },
        discount_percent,
        currency: row.string("currency"),
        status: row.string("status"),
    }
}

async fn fetch_models(db: &crate::db::Db) -> Result<Vec<ModelItem>, ApiError> {
    let rows = db
        .query_all(
            r#"
        SELECT id, app_type, COALESCE(model_pattern, canonical_name) AS model_pattern, display_name,
               COALESCE(status, 'active') AS status, COALESCE(is_public, 1) AS is_public,
               COALESCE(sort_order, 0) AS sort_order
          FROM models
         ORDER BY app_type, sort_order, model_pattern
        "#,
            vec![],
        )
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(model_item_from_row(db, row).await?);
    }
    Ok(out)
}

async fn fetch_model(db: &crate::db::Db, id: Uuid) -> Result<ModelItem, ApiError> {
    model_item_from_row(db, model_row(db, id).await?).await
}

async fn model_row(db: &crate::db::Db, id: Uuid) -> Result<crate::db::DbRow, ApiError> {
    db.query_optional(
        r#"
        SELECT id, app_type, COALESCE(model_pattern, canonical_name) AS model_pattern, display_name,
               COALESCE(status, 'active') AS status, COALESCE(is_public, 1) AS is_public,
               COALESCE(sort_order, 0) AS sort_order
          FROM models WHERE id=?1
        "#,
        vec![crate::db::uuid_val(id)],
    )
    .await?
    .ok_or_else(|| ApiError::bad_request("model_not_found", "model not found"))
}

async fn model_item_from_row(
    db: &crate::db::Db,
    row: crate::db::DbRow,
) -> Result<ModelItem, ApiError> {
    let id = row.uuid("id");
    let price = db
        .query_optional(
            r#"
        SELECT mp.id, mp.model_id, mp.app_type, mp.model_pattern, m.display_name, m.is_public,
               mp.input_per_million, mp.output_per_million, mp.cache_read_per_million,
               mp.cache_write_per_million, mp.currency, mp.status, COALESCE(vd.discount_percent, '10') AS discount_percent
          FROM model_prices mp LEFT JOIN models m ON m.id=mp.model_id
          LEFT JOIN model_vendor_discounts vd ON vd.app_type = mp.app_type
         WHERE mp.model_id=?1 AND mp.status='active'
         ORDER BY mp.effective_from DESC LIMIT 1
        "#,
            vec![crate::db::uuid_val(id)],
        )
        .await?
        .map(row_to_price);
    let routing = fetch_routing_rule(db, id).await?;
    let row_app_type = row.string("app_type");
    let row_app_type_alias = share_app_type_alias(&row_app_type);
    let support = share_support_flags(&row_app_type);
    let routeable_shares = db
        .query_one(
            r#"
        SELECT COUNT(*) AS count
          FROM router_shares
         WHERE (app_type IN (?1, ?2)
                OR (?3 = 1 AND enabled_codex = 1)
                OR (?4 = 1 AND enabled_claude = 1)
                OR (?5 = 1 AND enabled_gemini = 1))
           AND online=1 AND share_status='active'
           AND COALESCE(disabled_by_market, 0) = 0
        "#,
            vec![
                crate::db::val(&row_app_type),
                crate::db::val(row_app_type_alias),
                crate::db::val(support.codex),
                crate::db::val(support.claude),
                crate::db::val(support.gemini),
            ],
        )
        .await?
        .i64("count");
    Ok(ModelItem {
        id,
        app_type: row_app_type,
        model_pattern: row.string("model_pattern"),
        display_name: row.opt_string("display_name"),
        status: row.string("status"),
        is_public: row.bool("is_public"),
        sort_order: row.i64("sort_order"),
        price,
        routing,
        routeable_shares,
    })
}

async fn fetch_routing_rule(
    db: &crate::db::Db,
    model_id: Uuid,
) -> Result<Option<RoutingRuleItem>, ApiError> {
    let Some(row) = db.query_optional(
        "SELECT id, model_id, mode, priority, enabled, notes FROM model_routing_rules WHERE model_id=?1 LIMIT 1",
        vec![crate::db::uuid_val(model_id)],
    ).await? else {
        return Ok(None);
    };
    let rule_id = row.uuid("id");
    let shares = db.query_all(
        "SELECT router_id, share_id FROM model_routing_rule_shares WHERE rule_id=?1 ORDER BY router_id, share_id",
        vec![crate::db::uuid_val(rule_id)],
    ).await?.into_iter().map(|row| RoutingShareItem {
        router_id: row.string("router_id"),
        share_id: row.string("share_id"),
    }).collect();
    Ok(Some(RoutingRuleItem {
        id: rule_id,
        model_id,
        mode: row.string("mode"),
        priority: row.i64("priority"),
        enabled: row.bool("enabled"),
        notes: row.opt_string("notes"),
        shares,
    }))
}

async fn ensure_model(
    db: &crate::db::Db,
    app_type: &str,
    model_pattern: &str,
    display_name: Option<String>,
) -> Result<Uuid, ApiError> {
    validate_app_type(app_type)?;
    ensure_vendor_discount(db, app_type).await?;
    let now = crate::db::now_string();
    if let Some(row) = db.query_optional(
        "SELECT id FROM models WHERE app_type=?1 AND COALESCE(model_pattern, canonical_name)=?2 LIMIT 1",
        vec![crate::db::val(app_type), crate::db::val(model_pattern)],
    ).await? {
        return Ok(row.uuid("id"));
    }
    let id = Uuid::new_v4();
    db.execute(
        r#"
        INSERT INTO models (id, app_type, canonical_name, model_pattern, display_name, status, is_public, sort_order, aliases_json, metadata_json, created_at, updated_at)
        VALUES (?1,?2,?3,?3,?4,'active',?5,0,'[]','{}',?6,?6)
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::val(app_type),
            crate::db::val(model_pattern),
            crate::db::opt_val(display_name),
            crate::db::val(model_pattern != "*"),
            crate::db::val(now),
        ],
    ).await?;
    Ok(id)
}

async fn ensure_vendor_discount(db: &crate::db::Db, app_type: &str) -> Result<(), ApiError> {
    let now = crate::db::now_string();
    db.execute(
        r#"
        INSERT INTO model_vendor_discounts (app_type, discount_percent, updated_at)
        VALUES (?1, '10', ?2)
        ON CONFLICT(app_type) DO NOTHING
        "#,
        vec![crate::db::val(app_type), crate::db::val(now)],
    )
    .await?;
    Ok(())
}

async fn upsert_model_price_inner(
    state: &AppState,
    admin_email: &str,
    model_id: Uuid,
    input: ModelPriceInput,
) -> Result<(), ApiError> {
    let model = model_row(state.db(), model_id).await?;
    let app_type = model.string("app_type");
    let model_pattern = model.string("model_pattern");
    let input_per_million = input.input_per_million;
    let output_per_million = input.output_per_million;
    let cache_read_per_million = input.cache_read_per_million.unwrap_or(Decimal::ZERO);
    let cache_write_per_million = input.cache_write_per_million.unwrap_or(Decimal::ZERO);
    if [
        input_per_million,
        output_per_million,
        cache_read_per_million,
        cache_write_per_million,
    ]
    .iter()
    .any(|value| *value < Decimal::ZERO)
    {
        return Err(ApiError::bad_request(
            "invalid_price",
            "model prices must be zero or positive",
        ));
    }
    let old = state.db().query_optional(
        "SELECT id, app_type, model_pattern, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million, currency, status FROM model_prices WHERE model_id=?1 AND status='active' ORDER BY effective_from DESC LIMIT 1",
        vec![crate::db::uuid_val(model_id)],
    ).await?;
    let price_id = old
        .as_ref()
        .map(|row| row.uuid("id"))
        .unwrap_or_else(Uuid::new_v4);
    let now = crate::db::now_string();
    state.db().execute(
        r#"
        INSERT INTO model_prices
          (id, model_id, app_type, model_pattern, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million, status, effective_from, created_at, updated_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?9,?9)
        ON CONFLICT(id) DO UPDATE SET
          input_per_million=excluded.input_per_million,
          output_per_million=excluded.output_per_million,
          cache_read_per_million=excluded.cache_read_per_million,
          cache_write_per_million=excluded.cache_write_per_million,
          app_type=excluded.app_type,
          model_pattern=excluded.model_pattern,
          model_id=excluded.model_id,
          updated_at=excluded.updated_at
        "#,
        vec![
            crate::db::uuid_val(price_id),
            crate::db::uuid_val(model_id),
            crate::db::val(&app_type),
            crate::db::val(&model_pattern),
            crate::db::dec_val(input_per_million),
            crate::db::dec_val(output_per_million),
            crate::db::dec_val(cache_read_per_million),
            crate::db::dec_val(cache_write_per_million),
            crate::db::val(&now),
        ],
    ).await?;
    let new_snapshot = serde_json::json!({
        "id": price_id,
        "model_id": model_id,
        "app_type": app_type,
        "model_pattern": model_pattern,
        "input_per_million": input_per_million.to_string(),
        "output_per_million": output_per_million.to_string(),
        "cache_read_per_million": cache_read_per_million.to_string(),
        "cache_write_per_million": cache_write_per_million.to_string(),
        "status": "active",
    });
    state.db().execute(
        "INSERT INTO price_changes (id, price_id, old_snapshot, new_snapshot, admin_actor, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(price_id),
            old.map(price_snapshot_val).unwrap_or(libsql::Value::Null),
            crate::db::json_val(new_snapshot),
            crate::db::val(admin_email),
            crate::db::opt_val(input.reason),
            crate::db::val(now),
        ],
    ).await?;
    write_admin_audit(
        state,
        admin_email,
        "model.price",
        "model",
        model_id,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

async fn set_model_status(
    state: &AppState,
    admin_email: &str,
    id: Uuid,
    status: &str,
) -> Result<Json<ModelItem>, ApiError> {
    model_row(state.db(), id).await?;
    state
        .db()
        .execute(
            "UPDATE models SET status=?2, updated_at=?3 WHERE id=?1",
            vec![
                crate::db::uuid_val(id),
                crate::db::val(status),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    write_admin_audit(
        state,
        admin_email,
        if status == "active" {
            "model.activate"
        } else {
            "model.deactivate"
        },
        "model",
        id,
        serde_json::json!({}),
    )
    .await?;
    fetch_model(state.db(), id).await.map(Json)
}

async fn active_price_row_for_model(
    db: &crate::db::Db,
    model_id: Uuid,
) -> Result<crate::db::DbRow, ApiError> {
    db.query_optional(
        "SELECT id FROM model_prices WHERE model_id=?1 AND status='active' ORDER BY effective_from DESC LIMIT 1",
        vec![crate::db::uuid_val(model_id)],
    ).await?.ok_or_else(|| ApiError::bad_request("model_not_priced", "model is not priced"))
}

async fn existing_rule_id(db: &crate::db::Db, model_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    Ok(db
        .query_optional(
            "SELECT id FROM model_routing_rules WHERE model_id=?1 LIMIT 1",
            vec![crate::db::uuid_val(model_id)],
        )
        .await?
        .map(|row| row.uuid("id")))
}

async fn ensure_rule(db: &crate::db::Db, model_id: Uuid) -> Result<Uuid, ApiError> {
    if let Some(id) = existing_rule_id(db, model_id).await? {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    db.execute(
        "INSERT INTO model_routing_rules (id, model_id, mode, priority, enabled, created_at, updated_at) VALUES (?1,?2,'all',0,1,?3,?3)",
        vec![crate::db::uuid_val(id), crate::db::uuid_val(model_id), crate::db::val(now)],
    ).await?;
    Ok(id)
}

#[allow(dead_code)]
pub async fn route_candidates(
    db: &crate::db::Db,
    model_id: Uuid,
    app_type: &str,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let rule = fetch_routing_rule(db, model_id).await?;
    let app_type_alias = share_app_type_alias(app_type);
    let support = share_support_flags(app_type);
    let capability = share_capability(app_type);
    let mut sql = r#"
        SELECT router_id, share_id, COALESCE(owner_email, installation_owner_email) AS owner_email,
               app_type, active_requests, parallel_limit, online_rate_24h, priority,
               enabled_claude, enabled_codex, enabled_gemini, last_success_at, last_seen_at
               , raw_json, for_sale
          FROM router_shares
         WHERE (app_type IN (?1, ?3)
                OR (?4 = 1 AND enabled_codex = 1)
                OR (?5 = 1 AND enabled_claude = 1)
                OR (?6 = 1 AND enabled_gemini = 1))
           AND online=1 AND share_status='active'
           AND COALESCE(disabled_by_market, 0) = 0
           AND (parallel_limit = -1 OR active_requests < parallel_limit)
           AND COALESCE(owner_email, installation_owner_email) IS NOT NULL
           AND (last_error_at IS NULL OR last_error_at < ?2)
           AND NOT EXISTS (
             SELECT 1 FROM market_share_capability_blocks mscb
              WHERE mscb.router_id = router_shares.router_id
                AND mscb.share_id = router_shares.share_id
                AND mscb.capability = ?7
           )
        "#
    .to_string();
    let cooldown_cutoff = (chrono::Utc::now() - chrono::Duration::minutes(2)).to_rfc3339();
    let mut params = vec![
        crate::db::val(app_type),
        crate::db::val(cooldown_cutoff),
        crate::db::val(app_type_alias),
        crate::db::val(support.codex),
        crate::db::val(support.claude),
        crate::db::val(support.gemini),
        crate::db::val(capability),
    ];
    if let Some(rule) = &rule {
        if rule.enabled && rule.mode == "include_only" && rule.shares.is_empty() {
            return Ok(Vec::new());
        }
        if rule.enabled
            && (rule.mode == "include_only" || rule.mode == "exclude")
            && !rule.shares.is_empty()
        {
            let pairs = rule
                .shares
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    let base = params.len() + idx * 2 + 1;
                    format!("(router_id=?{base} AND share_id=?{})", base + 1)
                })
                .collect::<Vec<_>>()
                .join(" OR ");
            for share in &rule.shares {
                params.push(crate::db::val(&share.router_id));
                params.push(crate::db::val(&share.share_id));
            }
            if rule.mode == "include_only" {
                sql.push_str(&format!(" AND ({pairs})"));
            } else {
                sql.push_str(&format!(" AND NOT ({pairs})"));
            }
        }
    }
    sql.push_str(" ORDER BY active_requests ASC, priority DESC, CAST(online_rate_24h AS REAL) DESC, COALESCE(last_success_at, last_seen_at) DESC LIMIT 20");
    let rows = db.query_all(&sql, params).await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            crate::proxy::raw_share_app_token_sale_visible(
                row.opt_string("raw_json").as_deref(),
                capability,
                &row.string("for_sale"),
                None,
            )
        })
        .map(|row| row.to_json())
        .collect())
}

async fn route_diagnostics(
    db: &crate::db::Db,
    model_id: Uuid,
    app_type: &str,
) -> Result<serde_json::Value, ApiError> {
    let rule = fetch_routing_rule(db, model_id).await?;
    let app_type_alias = share_app_type_alias(app_type);
    let support = share_support_flags(app_type);
    let capability = share_capability(app_type);
    let now = chrono::Utc::now().to_rfc3339();
    let rows = db.query_all(
        r#"
        SELECT router_id, share_id, COALESCE(owner_email, installation_owner_email) AS owner_email,
               app_type, online, share_status, for_sale, active_requests, parallel_limit,
               online_rate_24h, priority, enabled_claude, enabled_codex, enabled_gemini,
               last_error_at, cooldown_until, failure_count,
               last_success_at, last_seen_at, raw_json
          FROM router_shares
         WHERE (app_type IN (?1, ?2)
                OR (?3 = 1 AND enabled_codex = 1)
                OR (?4 = 1 AND enabled_claude = 1)
                OR (?5 = 1 AND enabled_gemini = 1))
         ORDER BY active_requests ASC, priority DESC, CAST(online_rate_24h AS REAL) DESC, COALESCE(last_success_at, last_seen_at) DESC
        "#,
        vec![
            crate::db::val(app_type),
            crate::db::val(app_type_alias),
            crate::db::val(support.codex),
            crate::db::val(support.claude),
            crate::db::val(support.gemini),
        ],
    ).await?;
    let blocks = db.query_all(
        "SELECT router_id, share_id, reason, expires_at FROM model_share_blocks WHERE model_id=?1 AND expires_at>?2",
        vec![crate::db::uuid_val(model_id), crate::db::val(&now)],
    ).await?;
    let block_keys = blocks
        .iter()
        .map(|row| (row.string("router_id"), row.string("share_id")))
        .collect::<std::collections::HashSet<_>>();
    let capability_blocks = db.query_all(
        "SELECT router_id, share_id, capability, reason, created_by, created_at FROM market_share_capability_blocks WHERE capability=?1",
        vec![crate::db::val(capability)],
    ).await?;
    let capability_block_keys = capability_blocks
        .iter()
        .map(|row| (row.string("router_id"), row.string("share_id")))
        .collect::<std::collections::HashSet<_>>();
    let rule_keys = rule
        .as_ref()
        .map(|rule| {
            rule.shares
                .iter()
                .map(|s| (s.router_id.clone(), s.share_id.clone()))
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let mut base_candidates = Vec::new();
    let mut excluded_offline = Vec::new();
    let mut excluded_parallel_limit = Vec::new();
    let mut excluded_cooldown = Vec::new();
    let mut excluded_blocklist = Vec::new();
    let mut excluded_by_rule = Vec::new();
    let mut final_candidates = Vec::new();
    for row in rows {
        let key = (row.string("router_id"), row.string("share_id"));
        let json = row.to_json();
        if !row.bool("online")
            || row.string("share_status") != "active"
            || !crate::proxy::raw_share_app_token_sale_visible(
                row.opt_string("raw_json").as_deref(),
                capability,
                &row.string("for_sale"),
                None,
            )
            || row.string("owner_email").is_empty()
        {
            excluded_offline.push(json);
            continue;
        }
        base_candidates.push(json.clone());
        if row.i64("parallel_limit") != -1
            && row.i64("active_requests") >= row.i64("parallel_limit")
        {
            excluded_parallel_limit.push(json);
            continue;
        }
        if row
            .opt_string("cooldown_until")
            .is_some_and(|value| value > now)
        {
            excluded_cooldown.push(json);
            continue;
        }
        if block_keys.contains(&key) {
            excluded_blocklist.push(json);
            continue;
        }
        if capability_block_keys.contains(&key) {
            excluded_blocklist.push(json);
            continue;
        }
        if let Some(rule) = &rule {
            if rule.enabled && rule.mode == "include_only" && !rule_keys.contains(&key) {
                excluded_by_rule.push(json);
                continue;
            }
            if rule.enabled && rule.mode == "exclude" && rule_keys.contains(&key) {
                excluded_by_rule.push(json);
                continue;
            }
        }
        final_candidates.push(json);
    }
    Ok(serde_json::json!({
        "routing_rule": rule,
        "base_candidates": base_candidates,
        "excluded_offline": excluded_offline,
        "excluded_parallel_limit": excluded_parallel_limit,
        "excluded_cooldown": excluded_cooldown,
        "excluded_blocklist": excluded_blocklist,
        "excluded_by_rule": excluded_by_rule,
        "final_candidates": final_candidates,
        "selected_share": final_candidates.first(),
        "active_blocks": blocks.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
        "active_capability_blocks": capability_blocks.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
    }))
}

fn share_app_type_alias(app_type: &str) -> &str {
    match app_type {
        "openai" => "codex",
        "anthropic" => "claude",
        other => other,
    }
}

struct ShareSupportFlags {
    claude: bool,
    codex: bool,
    gemini: bool,
}

fn share_support_flags(app_type: &str) -> ShareSupportFlags {
    ShareSupportFlags {
        claude: app_type == "anthropic" || app_type == "claude",
        codex: app_type == "openai" || app_type == "codex",
        gemini: app_type == "gemini",
    }
}

fn share_capability(app_type: &str) -> &'static str {
    match app_type {
        "anthropic" | "claude" => "claude",
        "gemini" => "gemini",
        _ => "codex",
    }
}

fn validate_app_type(app_type: &str) -> Result<(), ApiError> {
    let valid = !app_type.trim().is_empty()
        && app_type.len() <= 64
        && app_type
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-');
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_app_type",
            "app_type must be 1-64 chars: lowercase letters, numbers, '-' or '_'",
        ))
    }
}

fn validate_status(status: &str) -> Result<(), ApiError> {
    if matches!(status, "active" | "inactive") {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_status",
            "invalid model status",
        ))
    }
}

fn validate_routing_mode(mode: &str) -> Result<(), ApiError> {
    if matches!(mode, "all" | "include_only" | "exclude") {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_routing_mode",
            "invalid routing mode",
        ))
    }
}

async fn write_admin_audit(
    state: &AppState,
    admin: &str,
    action: &str,
    reference_type: &str,
    reference_id: Uuid,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    state.db().execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(admin),
            crate::db::val(action),
            crate::db::val(reference_type),
            crate::db::uuid_val(reference_id),
            crate::db::json_val(metadata),
            crate::db::val(crate::db::now_string()),
        ],
    ).await?;
    Ok(())
}

fn official_price_field_missing(app_type: &str, _model_pattern: &str, field: &str) -> bool {
    // OpenAI publishes discounted cached input pricing, but no separate cache-write price.
    matches!(app_type, "openai" | "deepseek") && field == "cache_write"
}

fn price_change_json(row: crate::db::DbRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.string("id"),
        "price_id": row.opt_string("price_id"),
        "old_snapshot": row.opt_string("old_snapshot").and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()),
        "new_snapshot": row.opt_string("new_snapshot").and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()),
        "admin_actor": row.opt_string("admin_actor"),
        "reason": row.opt_string("reason"),
        "created_at": row.opt_string("created_at"),
    })
}

fn price_snapshot_val(row: crate::db::DbRow) -> libsql::Value {
    crate::db::json_val(serde_json::json!({
        "id": row.string("id"),
        "app_type": row.string("app_type"),
        "model_pattern": row.string("model_pattern"),
        "input_per_million": row.string("input_per_million"),
        "output_per_million": row.string("output_per_million"),
        "cache_read_per_million": row.string("cache_read_per_million"),
        "cache_write_per_million": row.string("cache_write_per_million"),
        "currency": row.string("currency"),
        "status": row.string("status"),
    }))
}
