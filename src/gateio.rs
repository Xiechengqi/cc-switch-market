use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::{Digest, Sha512};

use crate::{app_state::AppState, error::ApiError};

type HmacSha512 = Hmac<Sha512>;

#[derive(Serialize)]
pub struct GateioProof {
    pub attempt_id: uuid::Uuid,
    pub request_path: String,
    pub method: String,
    pub external_tx_id: String,
    pub gateio_batch_id: Option<String>,
    pub transfer_amount: String,
    pub currency: String,
    pub request_object_key: Option<String>,
    pub response_object_key: Option<String>,
    pub request_object_sha256: Option<String>,
    pub response_object_sha256: Option<String>,
}

pub async fn execute_transfer(
    state: &AppState,
    payout_id: uuid::Uuid,
    amount: rust_decimal::Decimal,
    target: serde_json::Value,
) -> Result<GateioProof, ApiError> {
    if !state.config.gateio_auto_payout_enabled || state.config.gateio_api_key.is_empty() {
        return Err(ApiError::service_unavailable(
            "Gate.io automatic payout is disabled or not configured",
        ));
    }

    let path = "/api/v4/withdrawals/push";
    let attempt_id = uuid::Uuid::new_v4();
    let receive_uid = gateio_receive_uid(&target)?;
    let body = serde_json::json!({
        "receive_uid": receive_uid,
        "currency": state.config.gateio_settlement_currency,
        "amount": amount.to_string(),
    });
    let body_string = body.to_string();
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let hashed_payload = hex::encode(Sha512::digest(body_string.as_bytes()));
    let sign_string = format!("POST\n{path}\n\n{hashed_payload}\n{timestamp}");
    let signature = sign(&state.config.gateio_api_secret, &sign_string)?;
    let request_object = state
        .object_store
        .put_json(
            format!("payouts/{payout_id}/gateio/{attempt_id}-request.json"),
            &serde_json::json!({
                "method": "POST",
                "path": path,
                "body": mask_gateio_body(&body),
                "timestamp": timestamp,
                "sign_payload_sha512": hex::encode(Sha512::digest(sign_string.as_bytes())),
            }),
        )
        .await?;
    crate::object_store::record_object_ref(
        state,
        &request_object,
        "payout_request",
        payout_id,
        "gateio_request",
        Some("application/json"),
    )
    .await?;

    let response = state
        .http
        .post(format!("{}{}", state.config.gateio_api_base, path))
        .header("KEY", &state.config.gateio_api_key)
        .header("Timestamp", timestamp)
        .header("SIGN", signature)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::service_unavailable(format!("gateio request failed: {e}")))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let parsed_response = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
    let response_object = state
        .object_store
        .put_json(
            format!("payouts/{payout_id}/gateio/{attempt_id}-response.json"),
            &serde_json::json!({
                "status": status.as_u16(),
                "body": parsed_response,
                "raw_body": if parsed_response.is_null() { Some(text.clone()) } else { None },
            }),
        )
        .await?;
    crate::object_store::record_object_ref(
        state,
        &response_object,
        "payout_request",
        payout_id,
        "gateio_response",
        Some("application/json"),
    )
    .await?;
    if !status.is_success() {
        return Err(ApiError::service_unavailable(format!(
            "gateio transfer failed: {status} {text}"
        )));
    }
    Ok(GateioProof {
        attempt_id,
        request_path: path.to_string(),
        method: "gateio".to_string(),
        external_tx_id: external_tx_id(&parsed_response).unwrap_or_else(|| payout_id.to_string()),
        gateio_batch_id: external_tx_id(&parsed_response),
        transfer_amount: amount.to_string(),
        currency: state.config.gateio_settlement_currency.clone(),
        request_object_key: Some(request_object.object_key),
        response_object_key: Some(response_object.object_key),
        request_object_sha256: Some(request_object.content_sha256),
        response_object_sha256: Some(response_object.content_sha256),
    })
}

pub async fn self_check(state: &AppState) -> Result<(), ApiError> {
    if !state.config.gateio_auto_payout_enabled {
        return Ok(());
    }
    if state.config.gateio_api_key.trim().is_empty()
        || state.config.gateio_api_secret.trim().is_empty()
    {
        return Err(ApiError::service_unavailable(
            "Gate.io automatic payout is enabled but key/secret are missing",
        ));
    }
    let path = "/api/v4/wallet/total_balance";
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let hashed_payload = hex::encode(Sha512::digest(b""));
    let sign_string = format!("GET\n{path}\n\n{hashed_payload}\n{timestamp}");
    let signature = sign(&state.config.gateio_api_secret, &sign_string)?;
    let response = state
        .http
        .get(format!("{}{}", state.config.gateio_api_base, path))
        .header("KEY", &state.config.gateio_api_key)
        .header("Timestamp", timestamp)
        .header("SIGN", signature)
        .send()
        .await
        .map_err(|e| ApiError::service_unavailable(format!("gateio self-check failed: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::service_unavailable(format!(
            "gateio self-check returned {}",
            response.status()
        )));
    }
    Ok(())
}

fn gateio_receive_uid(target: &serde_json::Value) -> Result<u64, ApiError> {
    let uid = match target.get("uid") {
        Some(value) if value.is_u64() => value.as_u64(),
        Some(value) if value.is_string() => value
            .as_str()
            .and_then(|value| value.trim().parse::<u64>().ok()),
        _ => None,
    };
    let Some(uid) = uid.filter(|uid| *uid > 0) else {
        return Err(ApiError::bad_request(
            "missing_gateio_uid",
            "Gate.io user UID is required for automatic payout",
        ));
    };
    Ok(uid)
}

fn mask_gateio_body(body: &serde_json::Value) -> serde_json::Value {
    body.clone()
}

fn external_tx_id(value: &serde_json::Value) -> Option<String> {
    for key in ["id", "txid", "tx_id", "withdrawal_id", "transfer_id"] {
        if let Some(id) = value.get(key).and_then(|value| value.as_str()) {
            return Some(id.to_string());
        }
    }
    None
}

fn sign(secret: &str, payload: &str) -> Result<String, ApiError> {
    let mut mac = HmacSha512::new_from_slice(secret.as_bytes())
        .map_err(|_| ApiError::bad_request("invalid_gateio_secret", "invalid Gate.io secret"))?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}
