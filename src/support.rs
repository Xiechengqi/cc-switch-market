use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{app_state::AppState, auth::Principal, error::ApiError};

#[derive(Deserialize)]
pub struct PresignAttachmentRequest {
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
}

#[derive(Serialize)]
pub struct PresignAttachmentResponse {
    pub attachment_id: Uuid,
    pub object_key: String,
    pub upload_url: String,
}

#[derive(Deserialize)]
pub struct CreateTicketRequest {
    pub ticket_type: String,
    pub subject: String,
    pub body_text: String,
    pub priority: Option<String>,
    pub attachment_ids: Option<Vec<Uuid>>,
    pub related_reference_type: Option<String>,
    pub related_reference_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct AddMessageRequest {
    pub body_text: String,
    pub attachment_ids: Option<Vec<Uuid>>,
}

#[derive(Serialize)]
pub struct TicketItem {
    pub id: Uuid,
    pub ticket_no: String,
    pub ticket_type: String,
    pub status: String,
    pub priority: String,
    pub subject: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub can_close: bool,
    pub can_delete: bool,
}

fn normalize_priority(value: Option<String>) -> String {
    match value.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        Some(ref v) if v == "low" => "low".to_string(),
        Some(ref v) if v == "high" => "high".to_string(),
        Some(ref v) if v == "urgent" => "urgent".to_string(),
        _ => "normal".to_string(),
    }
}

async fn ticket_can_modify_for_user(
    state: &AppState,
    ticket_id: Uuid,
    user_id: Uuid,
) -> Result<(bool, bool, String), ApiError> {
    let ticket = state
        .db()
        .query_optional(
            "SELECT status FROM tickets WHERE id = ?1 AND creator_user_id = ?2",
            vec![crate::db::uuid_val(ticket_id), crate::db::uuid_val(user_id)],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("ticket_not_found", "Ticket not found"))?;
    let status = ticket.string("status");
    let admin_replies = state
        .db()
        .query_one(
            "SELECT COUNT(*) AS c FROM ticket_messages WHERE ticket_id = ?1 AND sender_type = 'admin' AND internal_note = 0",
            vec![crate::db::uuid_val(ticket_id)],
        )
        .await?
        .i64("c");
    let is_terminal = matches!(status.as_str(), "closed" | "resolved");
    let can_close = !is_terminal;
    let can_delete = !is_terminal && admin_replies == 0;
    Ok((can_close, can_delete, status))
}

#[derive(Deserialize)]
pub struct TicketQuery {
    pub status: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn presign_attachment(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<PresignAttachmentRequest>,
) -> Result<Json<PresignAttachmentResponse>, ApiError> {
    if input.byte_size > 2 * 1024 * 1024 {
        return Err(ApiError::bad_request(
            "attachment_too_large",
            "max attachment size is 2MB",
        ));
    }
    if !input
        .content_type
        .to_ascii_lowercase()
        .starts_with("image/")
    {
        return Err(ApiError::bad_request(
            "attachment_invalid_type",
            "only image attachments are allowed",
        ));
    }
    let attachment_id = Uuid::new_v4();
    let safe_name = input.filename.replace('/', "_");
    let object_key = format!("support/unbound/{attachment_id}/{safe_name}");
    let encoded_key =
        url::form_urlencoded::byte_serialize(object_key.as_bytes()).collect::<String>();
    let upload_url = format!(
        "{}/market-api/object-upload/{}",
        state.config.market_public_base_url, encoded_key
    );
    state
        .db()
        .execute(
            r#"
            INSERT INTO ticket_attachments
              (id, uploader_type, uploader_user_id, uploader_email, object_key, content_sha256, content_type, byte_size, original_filename, created_at)
            VALUES (?1, 'user', ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8)
            "#,
            vec![
                crate::db::uuid_val(attachment_id),
                crate::db::uuid_val(principal.user_id),
                crate::db::val(&principal.email),
                crate::db::val(&object_key),
                crate::db::val(input.content_type),
                crate::db::val(input.byte_size),
                crate::db::val(input.filename),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    Ok(Json(PresignAttachmentResponse {
        attachment_id,
        object_key,
        upload_url,
    }))
}

pub async fn create_ticket(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<CreateTicketRequest>,
) -> Result<Json<TicketItem>, ApiError> {
    crate::rate_limit::check("ticket_create", &principal.user_id.to_string(), 12)?;
    let ticket_no = format!(
        "CSM-{}",
        Uuid::new_v4().simple().to_string()[..8].to_uppercase()
    );
    let tx = state.db().begin_immediate().await?;
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    let priority = normalize_priority(input.priority);
    tx.execute(
        r#"
        INSERT INTO tickets
          (id, ticket_no, ticket_type, status, priority, subject, creator_user_id, related_reference_type, related_reference_id, created_at, updated_at)
        VALUES (?1,?2,?3,'open',?4,?5,?6,?7,?8,?9,?9)
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::val(&ticket_no),
            crate::db::val(input.ticket_type),
            crate::db::val(priority),
            crate::db::val(input.subject),
            crate::db::uuid_val(principal.user_id),
            crate::db::opt_val(input.related_reference_type),
            crate::db::opt_uuid_val(input.related_reference_id),
            crate::db::val(&now),
        ],
    )
    .await?;
    let message_id = Uuid::new_v4();
    tx.execute(
        "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, created_at) VALUES (?1,?2,'user',?3,?4,?5)",
        vec![
            crate::db::uuid_val(message_id),
            crate::db::uuid_val(id),
            crate::db::val(principal.email),
            crate::db::val(input.body_text),
            crate::db::val(&now),
        ],
    )
    .await?;
    bind_attachments_locked(&tx, input.attachment_ids, principal.user_id, id, message_id).await?;
    tx.commit().await?;
    let row = state
        .db()
        .query_one(
            &format!("SELECT {TICKET_LIST_COLS} FROM tickets WHERE id = ?1"),
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    Ok(Json(row_to_ticket(row)))
}

pub async fn report_usage(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<TicketItem>, ApiError> {
    crate::rate_limit::check("usage_report_create", &principal.user_id.to_string(), 12)?;
    let charge = state
        .db()
        .query_optional(
            r#"
            SELECT rc.id, rc.request_id, rc.app_type, rc.model, rc.status,
                   rc.router_id, rc.share_id, rc.owner_email,
                   rc.reserved_amount, rc.usage_amount, rc.price_snapshot, rc.usage_json, rc.audit_flags,
                   rc.request_object_key, rc.request_object_sha256,
                   rc.response_meta_object_key, rc.response_meta_object_sha256,
                   rs.raw_json AS share_raw_json,
                   rc.created_at, rc.settled_at
              FROM request_charges rc
              LEFT JOIN router_shares rs ON rs.router_id = rc.router_id AND rs.share_id = rc.share_id
             WHERE rc.id = ?1 AND rc.user_id = ?2
            "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("usage_not_found", "usage record not found"))?;
    if charge.string("status") != "settled" {
        return Err(ApiError::bad_request(
            "usage_not_settled",
            "only settled usage records can be reported",
        ));
    }
    if let Some(existing) = state
        .db()
        .query_optional(
            "SELECT id FROM tickets WHERE creator_user_id = ?1 AND related_reference_type = 'request_charge' AND related_reference_id = ?2 AND ticket_type = 'usage_report' LIMIT 1",
            vec![
                crate::db::uuid_val(principal.user_id),
                crate::db::uuid_val(id),
            ],
        )
        .await?
    {
        let row = state
            .db()
            .query_one(
                &format!("SELECT {TICKET_LIST_COLS} FROM tickets WHERE id = ?1"),
                vec![crate::db::uuid_val(existing.uuid("id"))],
            )
            .await?;
        return Ok(Json(row_to_ticket(row)));
    }

    let ticket_no = format!(
        "CSM-{}",
        Uuid::new_v4().simple().to_string()[..8].to_uppercase()
    );
    let ticket_id = Uuid::new_v4();
    let message_id = Uuid::new_v4();
    let now = crate::db::now_string();
    let subject = format!("举报 API 调用 {}", charge.string("request_id"));
    let body_text = usage_report_body(&charge);
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        r#"
        INSERT INTO tickets
          (id, ticket_no, ticket_type, status, priority, subject, creator_user_id, related_reference_type, related_reference_id, created_at, updated_at)
        VALUES (?1,?2,'usage_report','open','high',?3,?4,'request_charge',?5,?6,?6)
        "#,
        vec![
            crate::db::uuid_val(ticket_id),
            crate::db::val(&ticket_no),
            crate::db::val(subject),
            crate::db::uuid_val(principal.user_id),
            crate::db::uuid_val(id),
            crate::db::val(&now),
        ],
    )
    .await?;
    tx.execute(
        "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, created_at) VALUES (?1,?2,'user',?3,?4,?5)",
        vec![
            crate::db::uuid_val(message_id),
            crate::db::uuid_val(ticket_id),
            crate::db::val(principal.email),
            crate::db::val(body_text),
            crate::db::val(&now),
        ],
    )
    .await?;
    tx.commit().await?;
    let row = state
        .db()
        .query_one(
            &format!("SELECT {TICKET_LIST_COLS} FROM tickets WHERE id = ?1"),
            vec![crate::db::uuid_val(ticket_id)],
        )
        .await?;
    Ok(Json(row_to_ticket(row)))
}

pub async fn list_tickets(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<TicketQuery>,
) -> Result<Json<crate::pagination::Page<TicketItem>>, ApiError> {
    let mut sql = format!("SELECT {TICKET_LIST_COLS} FROM tickets WHERE creator_user_id = ?1");
    let mut params = vec![crate::db::uuid_val(principal.user_id)];
    if let Some(status) = query.status {
        sql.push_str(&format!(" AND status = ?{}", params.len() + 1));
        params.push(crate::db::val(status));
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
    let rows = state.db().query_all(&sql, params).await?;
    let items = rows.into_iter().map(row_to_ticket).collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| item.created_at.to_rfc3339(),
    )))
}

fn usage_report_body(row: &crate::db::DbRow) -> String {
    let price_snapshot = compact_json_field(row, "price_snapshot");
    let usage_json = compact_json_field(row, "usage_json");
    let audit_flags = compact_json_field(row, "audit_flags");
    let share_subdomain =
        crate::proxy::share_subdomain(row.opt_string("share_raw_json").as_deref())
            .unwrap_or_default();
    format!(
        "用户举报一次已结算 API 调用，请协助核查。\n\n\
Request ID: {request_id}\n\
Charge ID: {charge_id}\n\
App Type: {app_type}\n\
Model: {model}\n\
Status: {status}\n\
Router: {router_id}\n\
Share: {share_id}\n\
Share Subdomain: {share_subdomain}\n\
Provider Owner: {owner_email}\n\
Reserved USD: {reserved_amount}\n\
Usage USD: {usage_amount}\n\
Created At: {created_at}\n\
Settled At: {settled_at}\n\
Request Object: {request_object_key}\n\
Request SHA256: {request_object_sha256}\n\
Response Meta Object: {response_meta_object_key}\n\
Response Meta SHA256: {response_meta_object_sha256}\n\n\
Usage JSON:\n{usage_json}\n\n\
Price Snapshot:\n{price_snapshot}\n\n\
Audit Flags:\n{audit_flags}",
        request_id = row.string("request_id"),
        charge_id = row.string("id"),
        app_type = row.string("app_type"),
        model = row.string("model"),
        status = row.string("status"),
        router_id = row.string("router_id"),
        share_id = row.string("share_id"),
        share_subdomain = share_subdomain,
        owner_email = row.string("owner_email"),
        reserved_amount = row.string("reserved_amount"),
        usage_amount = row
            .opt_string("usage_amount")
            .unwrap_or_else(|| "-".to_string()),
        created_at = row
            .opt_string("created_at")
            .unwrap_or_else(|| "-".to_string()),
        settled_at = row
            .opt_string("settled_at")
            .unwrap_or_else(|| "-".to_string()),
        request_object_key = row
            .opt_string("request_object_key")
            .unwrap_or_else(|| "-".to_string()),
        request_object_sha256 = row
            .opt_string("request_object_sha256")
            .unwrap_or_else(|| "-".to_string()),
        response_meta_object_key = row
            .opt_string("response_meta_object_key")
            .unwrap_or_else(|| "-".to_string()),
        response_meta_object_sha256 = row
            .opt_string("response_meta_object_sha256")
            .unwrap_or_else(|| "-".to_string()),
        usage_json = usage_json,
        price_snapshot = price_snapshot,
        audit_flags = audit_flags,
    )
}

fn compact_json_field(row: &crate::db::DbRow, key: &str) -> String {
    row.opt_string(key)
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| "-".to_string())
}

pub async fn get_ticket(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ticket = state
        .db()
        .query_one(
            &format!("SELECT {TICKET_LIST_COLS}, creator_user_id, closed_at, updated_at FROM tickets WHERE id = ?1 AND creator_user_id = ?2"),
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
            ],
        )
        .await?;
    let messages = state
        .db()
        .query_all(
            "SELECT * FROM ticket_messages WHERE ticket_id = ?1 AND internal_note = 0 ORDER BY created_at",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let attachments = ticket_attachments_json(&state, id).await?;
    Ok(Json(serde_json::json!({
        "ticket": serde_json::to_value(row_to_ticket(ticket.clone())).unwrap_or_else(|_| ticket.to_json()),
        "messages": messages.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "attachments": attachments
    })))
}

pub async fn add_ticket_message(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<AddMessageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rate_limit::check("ticket_message", &principal.user_id.to_string(), 30)?;
    let ticket = state
        .db()
        .query_optional(
            "SELECT id, status FROM tickets WHERE id = ?1 AND creator_user_id = ?2",
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("ticket_not_found", "Ticket not found"))?;
    let status = ticket.string("status");
    if matches!(status.as_str(), "closed" | "resolved") {
        return Err(ApiError::bad_request(
            "ticket_closed",
            "工单已关闭，不能继续回复",
        ));
    }
    let message_id = Uuid::new_v4();
    let now = crate::db::now_string();
    let tx = state.db().begin_immediate().await?;
    tx
        .execute(
            "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, created_at) VALUES (?1,?2,'user',?3,?4,?5)",
            vec![
                crate::db::uuid_val(message_id),
                crate::db::uuid_val(id),
                crate::db::val(&principal.email),
                crate::db::val(input.body_text),
                crate::db::val(&now),
            ],
        )
        .await?;
    bind_attachments_locked(&tx, input.attachment_ids, principal.user_id, id, message_id).await?;
    let next_status = if matches!(status.as_str(), "open") {
        "waiting_admin"
    } else if matches!(status.as_str(), "waiting_user") {
        "waiting_admin"
    } else {
        status.as_str()
    };
    tx.execute(
        "UPDATE tickets SET status=?2, updated_at=?3 WHERE id=?1",
        vec![
            crate::db::uuid_val(id),
            crate::db::val(next_status),
            crate::db::val(&now),
        ],
    )
    .await?;
    tx.commit().await?;
    let row = state
        .db()
        .query_one(
            "SELECT * FROM ticket_messages WHERE id = ?1",
            vec![crate::db::uuid_val(message_id)],
        )
        .await?;
    Ok(Json(row.to_json()))
}

pub async fn close_ticket(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (can_close, _, status) = ticket_can_modify_for_user(&state, id, principal.user_id).await?;
    if !can_close {
        return Err(ApiError::bad_request("ticket_already_closed", "工单已关闭"));
    }
    let now = crate::db::now_string();
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "UPDATE tickets SET status='closed', updated_at=?2, closed_at=?2 WHERE id=?1",
        vec![crate::db::uuid_val(id), crate::db::val(&now)],
    )
    .await?;
    tx.execute(
        "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, created_at) VALUES (?1,?2,'system',?3,?4,?5)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(id),
            crate::db::val(&principal.email),
            crate::db::val(format!("用户从 {status} 状态主动关闭工单。")),
            crate::db::val(&now),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true, "status": "closed" })))
}

pub async fn delete_ticket(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, can_delete, _) = ticket_can_modify_for_user(&state, id, principal.user_id).await?;
    if !can_delete {
        return Err(ApiError::bad_request(
            "ticket_not_deletable",
            "工单已经有管理员回复或已关闭，不能删除，只能关闭",
        ));
    }
    let attachments = state
        .db()
        .query_all(
            "SELECT object_key FROM ticket_attachments WHERE ticket_id = ?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    for row in attachments {
        if let Err(err) = state
            .object_store
            .delete_key(&row.string("object_key"))
            .await
        {
            tracing::warn!(error = %err, "delete ticket attachment object failed");
        }
    }
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "DELETE FROM ticket_attachments WHERE ticket_id = ?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM ticket_messages WHERE ticket_id = ?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.execute(
        "DELETE FROM tickets WHERE id = ?1 AND creator_user_id = ?2",
        vec![
            crate::db::uuid_val(id),
            crate::db::uuid_val(principal.user_id),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true, "deleted": true })))
}

pub async fn bind_attachments_locked(
    tx: &crate::db::DbTx,
    attachment_ids: Option<Vec<Uuid>>,
    uploader_user_id: Uuid,
    ticket_id: Uuid,
    message_id: Uuid,
) -> Result<(), ApiError> {
    for attachment_id in attachment_ids.unwrap_or_default() {
        let updated = tx.execute(
            "UPDATE ticket_attachments SET ticket_id = ?2, message_id = ?3, reference_type='ticket', reference_id=?2 WHERE id = ?1 AND ticket_id IS NULL AND uploader_user_id = ?4",
            vec![
                crate::db::uuid_val(attachment_id),
                crate::db::uuid_val(ticket_id),
                crate::db::uuid_val(message_id),
                crate::db::uuid_val(uploader_user_id),
            ],
        )
        .await?;
        if updated == 0 {
            return Err(ApiError::forbidden(
                "attachment does not belong to this user",
            ));
        }
    }
    Ok(())
}

fn row_to_ticket(row: crate::db::DbRow) -> TicketItem {
    let status = row.string("status");
    let admin_replies: i64 = row.i64("admin_reply_count");
    let is_terminal = matches!(status.as_str(), "closed" | "resolved");
    TicketItem {
        id: row.uuid("id"),
        ticket_no: row.string("ticket_no"),
        ticket_type: row.string("ticket_type"),
        status,
        priority: row.string("priority"),
        subject: row.string("subject"),
        created_at: row.datetime("created_at"),
        can_close: !is_terminal,
        can_delete: !is_terminal && admin_replies == 0,
    }
}

const TICKET_LIST_COLS: &str = "id, ticket_no, ticket_type, status, priority, subject, created_at, (SELECT COUNT(*) FROM ticket_messages WHERE ticket_id = tickets.id AND sender_type = 'admin' AND internal_note = 0) AS admin_reply_count";

pub async fn ticket_attachments_json(
    state: &AppState,
    ticket_id: Uuid,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let rows = state
        .db()
        .query_all(
            "SELECT * FROM ticket_attachments WHERE ticket_id = ?1 ORDER BY created_at",
            vec![crate::db::uuid_val(ticket_id)],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let object_key = row.string("object_key");
            let encoded_key =
                url::form_urlencoded::byte_serialize(object_key.as_bytes()).collect::<String>();
            let mut value = row.to_json();
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "download_url".to_string(),
                    serde_json::Value::String(format!(
                        "{}/market-api/object-download/{}",
                        state.config.market_public_base_url, encoded_key
                    )),
                );
            }
            value
        })
        .collect())
}
