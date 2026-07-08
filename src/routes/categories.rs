//! Per-user categories (prompt.md §9.5, §10, §11). The single grouping concept + digest bucket.
//! `Other` is the non-deletable catch-all; deleting any other category reassigns its feeds to it.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::seed::OTHER_CATEGORY;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/categories", get(list_categories).post(create_category))
        .route("/categories/:id", axum::routing::patch(update_category).delete(delete_category))
}

#[derive(Serialize)]
struct CategoryDto {
    id: i64,
    name: String,
    position: i64,
    feed_count: i64,
    deletable: bool,
}

/// `GET /api/categories` — the user's categories with feed counts (§9.5), scoped by session.
async fn list_categories(user: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<Vec<CategoryDto>>> {
    let rows = sqlx::query(
        "SELECT c.id, c.name, c.position,
                (SELECT COUNT(*) FROM subscriptions s WHERE s.category_id = c.id AND s.user_id = c.user_id) AS feed_count
         FROM categories c
         WHERE c.user_id = ?
         ORDER BY c.position, c.name",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    let cats = rows
        .into_iter()
        .map(|r| {
            let name: String = r.get("name");
            let deletable = name != OTHER_CATEGORY;
            CategoryDto { id: r.get("id"), name, position: r.get("position"), feed_count: r.get("feed_count"), deletable }
        })
        .collect();
    Ok(Json(cats))
}

#[derive(Deserialize)]
struct CreateCategory {
    name: String,
}

/// `POST /api/categories` — create a category (unique per user).
async fn create_category(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<CreateCategory>,
) -> ApiResult<Json<CategoryDto>> {
    let name = body.name.trim().to_string();
    validate_name(&name)?;

    let taken = sqlx::query("SELECT 1 FROM categories WHERE user_id = ? AND name = ? COLLATE NOCASE")
        .bind(user.id)
        .bind(&name)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
    if taken {
        return Err(AppError::Conflict("a category with that name already exists".into()));
    }

    let position: i64 = sqlx::query("SELECT COALESCE(MAX(position), 0) + 1 AS p FROM categories WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?
        .get("p");

    let id: i64 = sqlx::query("INSERT INTO categories (user_id, name, position) VALUES (?, ?, ?) RETURNING id")
        .bind(user.id)
        .bind(&name)
        .bind(position)
        .fetch_one(&state.pool)
        .await?
        .get("id");

    Ok(Json(CategoryDto { id, name, position, feed_count: 0, deletable: true }))
}

#[derive(Deserialize)]
struct UpdateCategory {
    name: Option<String>,
    position: Option<i64>,
}

/// `PATCH /api/categories/{id}` — rename and/or reorder. `Other` cannot be renamed (it's the
/// catch-all resolved by name).
async fn update_category(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateCategory>,
) -> ApiResult<Json<serde_json::Value>> {
    let current = category_name(&state, user.id, id).await?;

    if let Some(raw) = &body.name {
        let name = raw.trim().to_string();
        validate_name(&name)?;
        if current == OTHER_CATEGORY && name != OTHER_CATEGORY {
            return Err(AppError::Forbidden);
        }
        let clash = sqlx::query("SELECT 1 FROM categories WHERE user_id = ? AND name = ? COLLATE NOCASE AND id <> ?")
            .bind(user.id).bind(&name).bind(id)
            .fetch_optional(&state.pool).await?.is_some();
        if clash {
            return Err(AppError::Conflict("a category with that name already exists".into()));
        }
        sqlx::query("UPDATE categories SET name = ? WHERE id = ? AND user_id = ?")
            .bind(&name).bind(id).bind(user.id).execute(&state.pool).await?;
    }
    if let Some(pos) = body.position {
        sqlx::query("UPDATE categories SET position = ? WHERE id = ? AND user_id = ?")
            .bind(pos).bind(id).bind(user.id).execute(&state.pool).await?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/categories/{id}` — delete a category, reassigning its feeds to `Other`.
/// `Other` itself cannot be deleted (§11).
async fn delete_category(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let name = category_name(&state, user.id, id).await?;
    if name == OTHER_CATEGORY {
        return Err(AppError::Forbidden);
    }

    let other_id: i64 = sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = ?")
        .bind(user.id)
        .bind(OTHER_CATEGORY)
        .fetch_one(&state.pool)
        .await?
        .get("id");

    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE subscriptions SET category_id = ? WHERE user_id = ? AND category_id = ?")
        .bind(other_id).bind(user.id).bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM categories WHERE id = ? AND user_id = ?")
        .bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "ok": true, "reassigned_to": other_id })))
}

async fn category_name(state: &AppState, user_id: i64, id: i64) -> ApiResult<String> {
    sqlx::query("SELECT name FROM categories WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .map(|r| r.get("name"))
        .ok_or_else(|| AppError::NotFound("category not found".into()))
}

fn validate_name(name: &str) -> ApiResult<()> {
    let len = name.chars().count();
    if !(1..=40).contains(&len) {
        return Err(AppError::BadRequest("category name must be 1–40 characters".into()));
    }
    Ok(())
}
