use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    ledger::{self, AccountRef},
};

const STALE_RESERVED_REQUEST_MINUTES: i64 = 10;
const CLIENT_DISCONNECT_REVIEW_RELEASE_MINUTES: i64 = 5;
const STREAM_USAGE_MISSING_AUTO_RELEASE_FLAG: &str = "auto_released_stream_usage_missing";
const STALE_RESERVED_AUTO_RELEASE_FLAG: &str = "auto_released_stale_reserved";
const STALE_STREAMING_REVIEW_FLAG: &str = "auto_marked_stale_streaming";

pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(600);
        loop {
            if let Err(err) = cleanup_once(&state).await {
                tracing::warn!(error = %err, "maintenance cleanup failed");
            }
            tokio::time::sleep(interval).await;
        }
    })
}

async fn auto_close_stale_tickets(state: &AppState) -> anyhow::Result<()> {
    let cutoff = (Utc::now() - Duration::days(7)).to_rfc3339();
    let stale = state
        .db()
        .query_all(
            r#"
            SELECT t.id, t.status,
              (SELECT MAX(created_at) FROM ticket_messages tm
                WHERE tm.ticket_id = t.id AND tm.sender_type = 'admin' AND tm.internal_note = 0) AS last_admin_at,
              (SELECT MAX(created_at) FROM ticket_messages tm
                WHERE tm.ticket_id = t.id AND tm.sender_type = 'user') AS last_user_at
              FROM tickets t
             WHERE t.status = 'waiting_user'
            "#,
            vec![],
        )
        .await?;
    for row in stale {
        let last_admin = row.opt_string("last_admin_at");
        let last_user = row.opt_string("last_user_at");
        let Some(last_admin) = last_admin else {
            continue;
        };
        if let Some(user_at) = last_user.as_deref() {
            if user_at >= last_admin.as_str() {
                continue;
            }
        }
        if last_admin.as_str() >= cutoff.as_str() {
            continue;
        }
        let ticket_id = row.uuid("id");
        let now = crate::db::now_string();
        let tx = state.db().begin_immediate().await?;
        tx.execute(
            "UPDATE tickets SET status='closed', updated_at=?2, closed_at=?2 WHERE id=?1 AND status='waiting_user'",
            vec![crate::db::uuid_val(ticket_id), crate::db::val(&now)],
        )
        .await?;
        tx.execute(
            "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, internal_note, created_at) VALUES (?1,?2,'system',?3,?4,0,?5)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::uuid_val(ticket_id),
                crate::db::val("auto-close"),
                crate::db::val("用户超过 7 天未回复，工单已自动关闭。"),
                crate::db::val(&now),
            ],
        )
        .await?;
        tx.commit().await?;
        tracing::info!(%ticket_id, "auto-closed waiting_user ticket after 7 day inactivity");
    }
    Ok(())
}

async fn cleanup_once(state: &AppState) -> anyhow::Result<()> {
    let now = crate::db::now_string();
    state
        .db()
        .execute(
            "UPDATE web_sessions SET revoked_at=?1 WHERE revoked_at IS NULL AND expires_at < ?1",
            vec![crate::db::val(&now)],
        )
        .await?;

    let idempotency_cutoff = (Utc::now() - Duration::days(7)).to_rfc3339();
    state
        .db()
        .execute(
            "DELETE FROM request_idempotency WHERE completed_at IS NOT NULL AND completed_at < ?1",
            vec![crate::db::val(idempotency_cutoff)],
        )
        .await?;

    let attachment_cutoff = (Utc::now() - Duration::hours(24)).to_rfc3339();
    let stale_attachments = state
        .db()
        .query_all(
            "SELECT object_key FROM ticket_attachments WHERE ticket_id IS NULL AND created_at < ?1",
            vec![crate::db::val(&attachment_cutoff)],
        )
        .await?;
    for row in stale_attachments {
        if let Err(err) = state
            .object_store
            .delete_key(&row.string("object_key"))
            .await
        {
            tracing::warn!(error = %err, "delete stale attachment object failed");
        }
    }
    state
        .db()
        .execute(
            "DELETE FROM ticket_attachments WHERE ticket_id IS NULL AND created_at < ?1",
            vec![crate::db::val(attachment_cutoff)],
        )
        .await?;
    if let Err(err) = auto_close_stale_tickets(state).await {
        tracing::warn!(error = %err, "auto-close waiting_user tickets failed");
    }
    if let Err(err) = release_stale_reserved_charges(state).await {
        tracing::warn!(error = %err, "release stale reserved charges failed");
    }
    if let Err(err) = mark_stale_streaming_charges_for_review(state).await {
        tracing::warn!(error = %err, "mark stale streaming charges for review failed");
    }
    if let Err(err) = release_stream_usage_missing_reviews(state).await {
        tracing::warn!(error = %err, "release stream usage-missing reviews failed");
    }
    Ok(())
}

async fn release_stale_reserved_charges(state: &AppState) -> anyhow::Result<()> {
    let cutoff = (Utc::now() - Duration::minutes(STALE_RESERVED_REQUEST_MINUTES)).to_rfc3339();
    let stale = state
        .db()
        .query_all(
            r#"
            SELECT rc.id,
                   rc.request_id,
                   rc.user_id,
                   rc.reserved_amount,
                   rc.created_at,
                   CASE
                     WHEN NOT EXISTS (
                       SELECT 1
                         FROM request_attempts ra
                        WHERE ra.charge_id = rc.id
                     ) THEN 'no_request_attempt_after_reserve'
                     ELSE 'all_request_attempts_failed'
                   END AS release_reason
              FROM request_charges rc
             WHERE rc.status = 'reserved'
               AND rc.created_at < ?1
               AND (
                 NOT EXISTS (
                   SELECT 1
                     FROM request_attempts ra
                    WHERE ra.charge_id = rc.id
                 )
                 OR (
                   EXISTS (
                     SELECT 1
                       FROM request_attempts ra
                      WHERE ra.charge_id = rc.id
                   )
                   AND NOT EXISTS (
                     SELECT 1
                       FROM request_attempts ra
                      WHERE ra.charge_id = rc.id
                        AND ra.status <> 'error'
                   )
                 )
               )
             ORDER BY rc.created_at ASC
             LIMIT 100
            "#,
            vec![crate::db::val(&cutoff)],
        )
        .await?;

    for row in stale {
        let charge_id = row.uuid("id");
        let user_id = row.uuid("user_id");
        let request_id = row.string("request_id");
        let reserved_amount = row.decimal("reserved_amount");
        let release_reason = row.string("release_reason");
        match release_stale_reserved_charge(
            state,
            charge_id,
            user_id,
            reserved_amount,
            &release_reason,
        )
        .await
        {
            Ok(true) => tracing::warn!(
                %charge_id,
                %request_id,
                reserved_amount = %reserved_amount,
                created_at = %row.string("created_at"),
                release_reason = %release_reason,
                "released stale reserved request charge"
            ),
            Ok(false) => {}
            Err(err) => tracing::warn!(
                %charge_id,
                %request_id,
                error = %err,
                "failed to release stale reserved request charge"
            ),
        }
    }
    Ok(())
}

async fn release_stale_reserved_charge(
    state: &AppState,
    charge_id: Uuid,
    user_id: Uuid,
    reserved_amount: Decimal,
    release_reason: &str,
) -> anyhow::Result<bool> {
    let now = crate::db::now_string();
    let tx = state.db().begin_immediate().await?;
    let audit_flags = serde_json::json!([STALE_RESERVED_AUTO_RELEASE_FLAG, release_reason,]);
    let changed = tx
        .execute(
            r#"
            UPDATE request_charges
               SET status = 'failed_released',
                   audit_flags = ?2,
                   settled_at = ?3
             WHERE id = ?1
               AND status = 'reserved'
               AND (
                 NOT EXISTS (
                   SELECT 1
                     FROM request_attempts ra
                    WHERE ra.charge_id = request_charges.id
                 )
                 OR (
                   EXISTS (
                     SELECT 1
                       FROM request_attempts ra
                      WHERE ra.charge_id = request_charges.id
                   )
                   AND NOT EXISTS (
                     SELECT 1
                       FROM request_attempts ra
                      WHERE ra.charge_id = request_charges.id
                        AND ra.status <> 'error'
                   )
                 )
               )
            "#,
            vec![
                crate::db::uuid_val(charge_id),
                crate::db::json_val(audit_flags),
                crate::db::val(&now),
            ],
        )
        .await?;
    if changed == 0 {
        tx.commit().await?;
        return Ok(false);
    }
    ledger::transfer(
        &tx,
        AccountRef::User {
            account_type: "user_reserved",
            user_id,
        },
        AccountRef::User {
            account_type: "user_cash",
            user_id,
        },
        reserved_amount,
        "request_charge",
        charge_id,
        "system",
        Some("maintenance:stale-reserved"),
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

async fn mark_stale_streaming_charges_for_review(state: &AppState) -> anyhow::Result<()> {
    let cutoff = (Utc::now() - Duration::minutes(STALE_RESERVED_REQUEST_MINUTES)).to_rfc3339();
    let stale = state
        .db()
        .query_all(
            r#"
            SELECT rc.id, rc.request_id, rc.user_id, rc.audit_flags, rc.created_at
              FROM request_charges rc
             WHERE rc.status = 'streaming'
               AND rc.created_at < ?1
             ORDER BY rc.created_at ASC
             LIMIT 100
            "#,
            vec![crate::db::val(&cutoff)],
        )
        .await?;

    for row in stale {
        let charge_id = row.uuid("id");
        let request_id = row.string("request_id");
        let audit_flags = append_audit_flags(
            row.opt_string("audit_flags"),
            &[STALE_STREAMING_REVIEW_FLAG, "stream_usage_missing"],
        );
        match mark_stale_streaming_charge_for_review(state, charge_id, audit_flags).await {
            Ok(true) => tracing::warn!(
                %charge_id,
                %request_id,
                created_at = %row.string("created_at"),
                "marked stale streaming request charge for review"
            ),
            Ok(false) => {}
            Err(err) => tracing::warn!(
                %charge_id,
                %request_id,
                error = %err,
                "failed to mark stale streaming request charge for review"
            ),
        }
    }
    Ok(())
}

async fn mark_stale_streaming_charge_for_review(
    state: &AppState,
    charge_id: Uuid,
    audit_flags: serde_json::Value,
) -> anyhow::Result<bool> {
    let changed = state
        .db()
        .execute(
            r#"
            UPDATE request_charges
               SET status = 'needs_review',
                   usage_json = NULL,
                   audit_flags = ?2,
                   settled_at = ?3
             WHERE id = ?1
               AND status = 'streaming'
            "#,
            vec![
                crate::db::uuid_val(charge_id),
                crate::db::json_val(audit_flags),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    Ok(changed > 0)
}

async fn release_stream_usage_missing_reviews(state: &AppState) -> anyhow::Result<()> {
    let cutoff =
        (Utc::now() - Duration::minutes(CLIENT_DISCONNECT_REVIEW_RELEASE_MINUTES)).to_rfc3339();
    let reviews = state
        .db()
        .query_all(
            r#"
            SELECT rc.id, rc.request_id, rc.user_id, rc.reserved_amount, rc.audit_flags, rc.settled_at
              FROM request_charges rc
             WHERE rc.status = 'needs_review'
               AND rc.usage_json IS NULL
               AND rc.usage_amount IS NULL
               AND COALESCE(rc.settled_at, rc.created_at) < ?1
               AND EXISTS (
                 SELECT 1 FROM json_each(rc.audit_flags)
                  WHERE value = 'stream_usage_missing'
               )
             ORDER BY rc.created_at ASC
             LIMIT 100
            "#,
            vec![crate::db::val(&cutoff)],
        )
        .await?;

    for row in reviews {
        let charge_id = row.uuid("id");
        let user_id = row.uuid("user_id");
        let request_id = row.string("request_id");
        let reserved_amount = row.decimal("reserved_amount");
        let audit_flags = append_audit_flag(
            row.opt_string("audit_flags"),
            STREAM_USAGE_MISSING_AUTO_RELEASE_FLAG,
        );
        match release_stream_usage_missing_review(
            state,
            charge_id,
            user_id,
            reserved_amount,
            audit_flags,
        )
        .await
        {
            Ok(true) => tracing::warn!(
                %charge_id,
                %request_id,
                reserved_amount = %reserved_amount,
                settled_at = %row.opt_string("settled_at").unwrap_or_default(),
                "released stream usage-missing review charge"
            ),
            Ok(false) => {}
            Err(err) => tracing::warn!(
                %charge_id,
                %request_id,
                error = %err,
                "failed to release stream usage-missing review charge"
            ),
        }
    }
    Ok(())
}

async fn release_stream_usage_missing_review(
    state: &AppState,
    charge_id: Uuid,
    user_id: Uuid,
    reserved_amount: Decimal,
    audit_flags: serde_json::Value,
) -> anyhow::Result<bool> {
    let now = crate::db::now_string();
    let tx = state.db().begin_immediate().await?;
    let changed = tx
        .execute(
            r#"
            UPDATE request_charges
               SET status = 'failed_released',
                   audit_flags = ?2,
                   settled_at = ?3
             WHERE id = ?1
               AND status = 'needs_review'
               AND usage_json IS NULL
               AND usage_amount IS NULL
               AND EXISTS (
                 SELECT 1 FROM json_each(audit_flags)
                  WHERE value = 'stream_usage_missing'
               )
            "#,
            vec![
                crate::db::uuid_val(charge_id),
                crate::db::json_val(audit_flags),
                crate::db::val(&now),
            ],
        )
        .await?;
    if changed == 0 {
        tx.commit().await?;
        return Ok(false);
    }
    ledger::transfer(
        &tx,
        AccountRef::User {
            account_type: "user_reserved",
            user_id,
        },
        AccountRef::User {
            account_type: "user_cash",
            user_id,
        },
        reserved_amount,
        "request_charge",
        charge_id,
        "system",
        Some("maintenance:stream-usage-missing"),
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

fn append_audit_flag(current: Option<String>, flag: &str) -> serde_json::Value {
    append_audit_flags(current, &[flag])
}

fn append_audit_flags(current: Option<String>, flags_to_add: &[&str]) -> serde_json::Value {
    let mut flags = current
        .as_deref()
        .and_then(|value| serde_json::from_str::<Vec<serde_json::Value>>(value).ok())
        .unwrap_or_default();
    for flag in flags_to_add {
        if !flags.iter().any(|value| value.as_str() == Some(*flag)) {
            flags.push(serde_json::Value::String((*flag).to_string()));
        }
    }
    serde_json::Value::Array(flags)
}
