//! Admin handlers: usage stats, key management, provider status.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ai::provider::ProviderKind, database::ProviderRow, error::AppError, response::ApiResponse,
    state::AppState,
};

/// Overall usage summary for the dashboard.
#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub today: TodayUsage,
    pub by_key: Vec<KeyUsageDto>,
    pub by_provider: Vec<ProviderUsageDto>,
    pub by_day: Vec<DayUsageDto>,
    pub daily_quota_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct TodayUsage {
    pub requests: i64,
    pub total_tokens: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct KeyUsageDto {
    pub key_id: String,
    pub key_name: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub quota_daily_tokens: i64,
    pub used_today: i64,
}

#[derive(Debug, Serialize)]
pub struct ProviderUsageDto {
    pub provider: String,
    pub requests: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct DayUsageDto {
    pub day: String,
    pub tokens: i64,
}

#[derive(Debug, Deserialize)]
pub struct UsageLogQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub quota_daily_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateKeyRequest {
    pub quota_daily_tokens: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct CreateKeyResponse {
    pub id: String,
    pub name: String,
    /// Shown only once, at creation time.
    pub key: String,
    pub quota_daily_tokens: i64,
}

#[derive(Debug, Deserialize)]
pub struct ToggleProviderRequest {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    #[serde(default = "default_groq_kind")]
    pub kind: String,
    pub name: Option<String>,
    #[serde(alias = "key")]
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderStatusDto {
    #[serde(flatten)]
    pub row: ProviderRow,
    pub healthy: bool,
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateProviderResponse {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
}

fn default_groq_kind() -> String {
    "groq".to_string()
}

/// `GET /api/v1/admin/usage/summary`
pub async fn usage_summary(
    State(state): State<AppState>,
) -> Result<ApiResponse<UsageSummary>, AppError> {
    let db = &state.db;

    let today_rows = db.usage_log(10_000, 0).await?;
    let today_req = today_rows.len() as i64;
    let prompt: i64 = today_rows.iter().map(|r| r.prompt_tokens).sum();
    let completion: i64 = today_rows.iter().map(|r| r.completion_tokens).sum();

    let by_key_raw = db.usage_summary_today().await?;
    let keys = db.list_keys().await?;
    let by_key = by_key_raw
        .iter()
        .map(|k| {
            let key = keys.iter().find(|x| x.id == k.api_key_id);
            KeyUsageDto {
                key_id: k.api_key_id.clone(),
                key_name: k.api_key_name.clone(),
                requests: k.requests,
                total_tokens: k.total_tokens,
                quota_daily_tokens: key.map(|x| x.quota_daily_tokens).unwrap_or(0),
                used_today: k.total_tokens,
            }
        })
        .collect::<Vec<_>>();

    let by_provider = db
        .provider_usage_today()
        .await?
        .into_iter()
        .map(|p| ProviderUsageDto {
            provider: p.provider,
            requests: p.requests,
            total_tokens: p.total_tokens,
        })
        .collect::<Vec<_>>();

    let by_day = db
        .usage_by_day(7)
        .await?
        .into_iter()
        .map(|(day, tokens)| DayUsageDto { day, tokens })
        .collect::<Vec<_>>();

    Ok(ApiResponse::success(UsageSummary {
        today: TodayUsage {
            requests: today_req,
            total_tokens: prompt + completion,
            prompt_tokens: prompt,
            completion_tokens: completion,
        },
        by_key,
        by_provider,
        by_day,
        daily_quota_tokens: state.config.router.daily_quota_tokens,
    }))
}

/// `GET /api/v1/admin/usage/log?limit=50&offset=0`
pub async fn usage_log(
    State(state): State<AppState>,
    Query(q): Query<UsageLogQuery>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = state.db.usage_log(limit, offset).await?;

    let payload = serde_json::json!({
        "rows": rows.iter().map(|r| {
            serde_json::json!({
                "id": r.id,
                "key_name": r.api_key_name,
                "model": r.model,
                "provider": r.provider,
                "prompt_tokens": r.prompt_tokens,
                "completion_tokens": r.completion_tokens,
                "total_tokens": r.total_tokens,
                "latency_ms": r.latency_ms,
                "status": r.status,
                "created_at": r.created_at.to_rfc3339(),
            })
        }).collect::<Vec<_>>(),
        "limit": limit,
        "offset": offset,
    });
    Ok(ApiResponse::success(payload))
}

/// `GET /api/v1/admin/keys`
pub async fn list_keys(
    State(state): State<AppState>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let keys = state.db.list_keys().await?;
    let payload = keys
        .iter()
        .map(|k| {
            serde_json::json!({
                "id": k.id,
                "name": k.name,
                "key_prefix": format!("{}…", &k.key_hash[..12]),
                "quota_daily_tokens": k.quota_daily_tokens,
                "enabled": k.enabled,
                "created_at": k.created_at.to_rfc3339(),
            })
        })
        .collect::<Vec<_>>();
    Ok(ApiResponse::success(payload))
}

/// `POST /api/v1/admin/keys`
pub async fn create_key(
    State(state): State<AppState>,
    Json(payload): Json<CreateKeyRequest>,
) -> Result<(StatusCode, ApiResponse<CreateKeyResponse>), AppError> {
    if payload.name.trim().is_empty() {
        return Err(AppError::Validation("name is required".to_string()));
    }
    let quota = payload.quota_daily_tokens.unwrap_or(0).max(0);
    let key = format!("sk-router-{}", Uuid::new_v4().simple());
    let record = state
        .db
        .insert_key(payload.name.trim(), &key, quota)
        .await?;

    Ok((
        StatusCode::CREATED,
        ApiResponse::success(CreateKeyResponse {
            id: record.id,
            name: record.name,
            key,
            quota_daily_tokens: record.quota_daily_tokens,
        }),
    ))
}

/// `PATCH /api/v1/admin/keys/{id}`
pub async fn update_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateKeyRequest>,
) -> Result<ApiResponse<()>, AppError> {
    state
        .db
        .update_key(&id, payload.quota_daily_tokens, payload.enabled)
        .await?;
    Ok(ApiResponse::message("key updated"))
}

/// `DELETE /api/v1/admin/keys/{id}`
pub async fn delete_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse<()>, AppError> {
    state.db.delete_key(&id).await?;
    Ok(ApiResponse::message("key deleted"))
}

/// `GET /api/v1/admin/providers`
pub async fn list_providers(
    State(state): State<AppState>,
) -> Result<ApiResponse<Vec<ProviderStatusDto>>, AppError> {
    let rows = state.db.list_providers().await?;
    let runtime_providers = state.router.providers().await;
    let mut out = Vec::new();
    for row in rows {
        let provider = runtime_providers.iter().find(|p| p.id == row.id);
        let available = provider.map(|p| p.is_available()).unwrap_or(false);
        let runtime_enabled = provider.map(|p| p.enabled).unwrap_or(row.enabled);
        let enabled = runtime_enabled && row.enabled;
        let healthy = row
            .cooldown_until
            .map(|t| t <= chrono::Utc::now())
            .unwrap_or(true)
            && enabled
            && available;
        out.push(ProviderStatusDto {
            row: ProviderRow { enabled, ..row },
            healthy,
            available,
        });
    }
    Ok(ApiResponse::success(out))
}

/// `POST /api/v1/admin/providers/{id}/toggle`
pub async fn toggle_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ToggleProviderRequest>,
) -> Result<ApiResponse<()>, AppError> {
    if !state.router.set_enabled(&id, payload.enabled).await {
        return Err(AppError::NotFound);
    }
    Ok(ApiResponse::message("provider updated"))
}

/// `POST /api/v1/admin/providers` — add an encrypted Groq key from the dashboard.
pub async fn create_provider(
    State(state): State<AppState>,
    Json(payload): Json<CreateProviderRequest>,
) -> Result<(StatusCode, ApiResponse<CreateProviderResponse>), AppError> {
    let Some(kind) = ProviderKind::parse(&payload.kind) else {
        return Err(AppError::Validation(
            "kind must be groq, opencode, codex, claude, or agy".to_string(),
        ));
    };
    if kind != ProviderKind::Groq {
        return Err(AppError::Validation(
            "local agent CLIs are detected automatically; dashboard creation is for Groq keys"
                .to_string(),
        ));
    }

    let api_key = payload
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Validation("api_key is required".to_string()))?;
    let base_url = payload
        .base_url
        .unwrap_or_else(|| state.config.router.groq_base_url.clone());
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(AppError::Validation(
            "base_url must start with http:// or https://".to_string(),
        ));
    }
    let model = payload
        .model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| state.config.router.default_model.clone());
    let id = format!("groq-dashboard-{}", Uuid::new_v4().simple());
    let name = payload
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Groq {}", &id[id.len().saturating_sub(8)..]));

    state
        .db
        .insert_provider(
            &id,
            "groq",
            name.trim(),
            &base_url,
            Some(api_key),
            None,
            &model,
        )
        .await?;
    if !state.router.reload_provider(&id).await? {
        return Err(AppError::Internal(anyhow::anyhow!(
            "provider was stored but could not be loaded"
        )));
    }

    Ok((
        StatusCode::CREATED,
        ApiResponse::success(CreateProviderResponse {
            id,
            kind: "groq".to_string(),
            name: name.trim().to_string(),
            base_url,
            model,
        }),
    ))
}

/// `PATCH /api/v1/admin/providers/{id}` — rotate a key or edit metadata.
pub async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateProviderRequest>,
) -> Result<ApiResponse<()>, AppError> {
    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let base_url = payload
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if let Some(value) = base_url
        && !(value.starts_with("http://") || value.starts_with("https://"))
    {
        return Err(AppError::Validation(
            "base_url must start with http:// or https://".to_string(),
        ));
    }
    if let Some(value) = payload.api_key.as_deref()
        && value.trim().is_empty()
    {
        return Err(AppError::Validation("api_key cannot be empty".to_string()));
    }

    let changed = state
        .db
        .update_provider(
            &id,
            name,
            base_url,
            payload.api_key.as_deref(),
            payload.command.as_deref(),
            payload.model.as_deref(),
        )
        .await?;
    if !changed {
        return Err(AppError::NotFound);
    }
    state.router.reload_provider(&id).await?;
    Ok(ApiResponse::message("provider updated"))
}

/// `DELETE /api/v1/admin/providers/{id}`.
pub async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse<()>, AppError> {
    if !state.db.delete_provider(&id).await? {
        return Err(AppError::NotFound);
    }
    state.router.remove_provider(&id).await;
    Ok(ApiResponse::message("provider deleted"))
}
