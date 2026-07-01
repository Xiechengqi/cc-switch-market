use std::path::{Path, PathBuf};

use anyhow::Context;
use axum::{
    Json,
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, header},
    response::IntoResponse,
};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{app_state::AppState, auth::Principal, error::ApiError};

#[derive(Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredObject {
    pub object_key: String,
    pub content_sha256: String,
    pub byte_size: u64,
}

impl ObjectStore {
    pub fn local(root: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create object store dir {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root_for_log(&self) -> String {
        self.root.display().to_string()
    }

    pub async fn put_json(
        &self,
        object_key: impl AsRef<str>,
        value: &serde_json::Value,
    ) -> anyhow::Result<StoredObject> {
        let bytes = serde_json::to_vec_pretty(value).context("serialize object json")?;
        self.put_bytes(object_key, &bytes).await
    }

    pub async fn put_json_once(
        &self,
        object_key: impl AsRef<str>,
        value: &serde_json::Value,
    ) -> anyhow::Result<StoredObject> {
        let bytes = serde_json::to_vec_pretty(value).context("serialize object json")?;
        self.put_bytes_once(object_key, &bytes).await
    }

    pub async fn put_bytes(
        &self,
        object_key: impl AsRef<str>,
        bytes: &[u8],
    ) -> anyhow::Result<StoredObject> {
        let object_key = clean_object_key(object_key.as_ref())?;
        let path = self.path_for_key(&object_key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create object dir {}", parent.display()))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .with_context(|| format!("write object {}", path.display()))?;
        let content_sha256 = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
        Ok(StoredObject {
            object_key,
            content_sha256,
            byte_size: bytes.len() as u64,
        })
    }

    pub async fn put_bytes_once(
        &self,
        object_key: impl AsRef<str>,
        bytes: &[u8],
    ) -> anyhow::Result<StoredObject> {
        let object_key = clean_object_key(object_key.as_ref())?;
        let path = self.path_for_key(&object_key)?;
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            let existing = tokio::fs::read(&path)
                .await
                .with_context(|| format!("read existing object {}", path.display()))?;
            let content_sha256 = format!("sha256:{}", hex::encode(Sha256::digest(&existing)));
            return Ok(StoredObject {
                object_key,
                content_sha256,
                byte_size: existing.len() as u64,
            });
        }
        self.put_bytes(object_key, bytes).await
    }

    pub async fn read_bytes(&self, object_key: &str) -> anyhow::Result<Vec<u8>> {
        let path = self.path_for_key(object_key)?;
        tokio::fs::read(&path)
            .await
            .with_context(|| format!("read object {}", path.display()))
    }

    pub async fn delete_key(&self, object_key: &str) -> anyhow::Result<()> {
        let path = self.path_for_key(object_key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("delete object {}", path.display())),
        }
    }

    pub async fn remove_empty_parent_dirs(&self, object_key: &str) -> anyhow::Result<()> {
        let path = self.path_for_key(object_key)?;
        let Some(parent) = path.parent() else {
            return Ok(());
        };
        let mut current = parent.to_path_buf();
        while current != self.root {
            match tokio::fs::remove_dir(&current).await {
                Ok(()) => {}
                Err(err) => {
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) {
                        break;
                    }
                    return Err(err)
                        .with_context(|| format!("remove empty object dir {}", current.display()));
                }
            }
            let Some(parent) = current.parent() else {
                break;
            };
            current = parent.to_path_buf();
        }
        Ok(())
    }

    pub async fn health_check(&self) -> bool {
        let key = format!("health/{}.txt", Uuid::new_v4());
        self.put_bytes(&key, Utc::now().to_rfc3339().as_bytes())
            .await
            .is_ok()
    }

    fn path_for_key(&self, object_key: &str) -> anyhow::Result<PathBuf> {
        let clean = clean_object_key(object_key)?;
        Ok(self.root.join(clean))
    }
}

pub async fn record_object_ref(
    state: &AppState,
    stored: &StoredObject,
    reference_type: &str,
    reference_id: Uuid,
    object_role: &str,
    content_type: Option<&str>,
) -> Result<(), ApiError> {
    state
        .db()
        .execute(
            r#"
            INSERT INTO object_refs
              (id, object_key, content_sha256, byte_size, reference_type, reference_id, object_role, content_type, created_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
            ON CONFLICT(object_key) DO UPDATE SET
              content_sha256 = excluded.content_sha256,
              byte_size = excluded.byte_size,
              reference_type = excluded.reference_type,
              reference_id = excluded.reference_id,
              object_role = excluded.object_role,
              content_type = excluded.content_type
            "#,
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(&stored.object_key),
                crate::db::val(&stored.content_sha256),
                crate::db::val(stored.byte_size as i64),
                crate::db::val(reference_type),
                crate::db::uuid_val(reference_id),
                crate::db::val(object_role),
                crate::db::opt_val(content_type.map(ToOwned::to_owned)),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    Ok(())
}

pub async fn upload_object(
    State(state): State<AppState>,
    principal: Principal,
    AxumPath(object_key): AxumPath<String>,
    body: Bytes,
) -> Result<Json<StoredObject>, ApiError> {
    if body.len() > 10 * 1024 * 1024 {
        return Err(ApiError::bad_request(
            "object_too_large",
            "max object size is 10MB",
        ));
    }
    let clean_key = clean_object_key(&object_key)?;
    let attachment_id = support_attachment_id(&clean_key).ok_or_else(|| {
        ApiError::bad_request(
            "unsupported_object_upload",
            "object uploads are limited to presigned ticket attachments",
        )
    })?;
    let attachment = state
        .db()
        .query_optional(
            r#"
            SELECT id, object_key, byte_size
              FROM ticket_attachments
             WHERE id = ?1
               AND uploader_type = 'user'
               AND uploader_user_id = ?2
               AND ticket_id IS NULL
             LIMIT 1
            "#,
            vec![
                crate::db::uuid_val(attachment_id),
                crate::db::uuid_val(principal.user_id),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::forbidden("object upload is not owned by this user"))?;
    if attachment.string("object_key") != clean_key {
        return Err(ApiError::forbidden(
            "object upload key does not match presigned attachment",
        ));
    }
    let declared_size = attachment.i64("byte_size").max(0) as usize;
    if body.len() > declared_size {
        return Err(ApiError::bad_request(
            "object_too_large",
            "uploaded object exceeds declared attachment size",
        ));
    }
    let stored = state.object_store.put_bytes(&clean_key, &body).await?;
    state
        .db()
        .execute(
            r#"
            UPDATE ticket_attachments
               SET content_sha256 = ?2, byte_size = ?3
             WHERE id = ?1 AND uploader_type = 'user' AND uploader_user_id = ?4
            "#,
            vec![
                crate::db::uuid_val(attachment_id),
                crate::db::val(&stored.content_sha256),
                crate::db::val(stored.byte_size as i64),
                crate::db::uuid_val(principal.user_id),
            ],
        )
        .await?;
    record_object_ref(
        &state,
        &stored,
        "ticket_attachment",
        attachment_id,
        "attachment",
        None,
    )
    .await?;
    Ok(Json(stored))
}

pub async fn download_object(
    State(state): State<AppState>,
    principal: Principal,
    AxumPath(object_key): AxumPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    let clean_key = clean_object_key(&object_key)?;
    if !principal.is_admin {
        let allowed = state
            .db()
            .query_optional(
                r#"
                SELECT ta.id
                  FROM ticket_attachments ta
                  JOIN tickets t ON t.id = ta.ticket_id
                 WHERE ta.object_key = ?1 AND t.creator_user_id = ?2
                 LIMIT 1
                "#,
                vec![
                    crate::db::val(&clean_key),
                    crate::db::uuid_val(principal.user_id),
                ],
            )
            .await?
            .is_some();
        if !allowed {
            return Err(ApiError::forbidden("object access denied"));
        }
    }
    let bytes = state.object_store.read_bytes(&clean_key).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            clean_key.rsplit('/').next().unwrap_or("object")
        ))
        .map_err(|_| ApiError::service_unavailable("invalid object filename"))?,
    );
    Ok((headers, bytes))
}

fn support_attachment_id(object_key: &str) -> Option<Uuid> {
    let mut parts = object_key.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("support"), Some("unbound"), Some(id)) => Uuid::parse_str(id).ok(),
        _ => None,
    }
}

fn clean_object_key(value: &str) -> anyhow::Result<String> {
    let path = Path::new(value);
    if path.is_absolute()
        || value.contains("..")
        || value.starts_with('/')
        || value.trim().is_empty()
    {
        anyhow::bail!("invalid object key");
    }
    Ok(value
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/"))
}
