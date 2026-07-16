//! Daily/monthly token budget guard (prompt.md §6 "token budget guard", §11 "budget exceeded").
//! Usage counters live in `app_settings` as `period:count` so they reset when the period rolls.
//! A budget of `0` means unlimited.

use anyhow::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use super::AiParams;

const DAY_KEY: &str = "ai.usage_day";
const MONTH_KEY: &str = "ai.usage_month";

/// How many tokens have been spent in the current day / month (0 if the period rolled over).
pub async fn spent(pool: &SqlitePool) -> Result<(i64, i64)> {
    let now = Utc::now();
    let day = now.format("%Y-%m-%d").to_string();
    let month = now.format("%Y-%m").to_string();
    Ok((read_period(pool, DAY_KEY, &day).await, read_period(pool, MONTH_KEY, &month).await))
}

/// Reject the call up front if either budget is already exhausted (prompt.md §6). Returns a clear,
/// key-free message for the on-demand error path.
pub async fn check(pool: &SqlitePool, params: &AiParams) -> Result<(), String> {
    let (day, month) = spent(pool).await.map_err(|_| "could not read AI token usage".to_string())?;
    if params.daily_token_budget > 0 && day >= params.daily_token_budget {
        return Err(format!(
            "daily AI token budget exhausted ({day}/{} tokens used)",
            params.daily_token_budget
        ));
    }
    if params.monthly_token_budget > 0 && month >= params.monthly_token_budget {
        return Err(format!(
            "monthly AI token budget exhausted ({month}/{} tokens used)",
            params.monthly_token_budget
        ));
    }
    Ok(())
}

/// Add `tokens` to the current day + month counters (best-effort; a failure here never fails the
/// summarize call itself).
pub async fn record(pool: &SqlitePool, tokens: i64) {
    if tokens <= 0 {
        return;
    }
    let now = Utc::now();
    let day = now.format("%Y-%m-%d").to_string();
    let month = now.format("%Y-%m").to_string();
    let _ = bump(pool, DAY_KEY, &day, tokens).await;
    let _ = bump(pool, MONTH_KEY, &month, tokens).await;
}

async fn read_period(pool: &SqlitePool, key: &str, period: &str) -> i64 {
    let raw: Option<String> = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| r.get("value"));
    match raw.and_then(|v| parse_period(&v)) {
        Some((p, count)) if p == period => count,
        _ => 0,
    }
}

async fn bump(pool: &SqlitePool, key: &str, period: &str, tokens: i64) -> Result<()> {
    let current = read_period(pool, key, period).await;
    let value = format!("{period}:{}", current + tokens);
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

fn parse_period(v: &str) -> Option<(String, i64)> {
    let (period, count) = v.rsplit_once(':')?;
    Some((period.to_string(), count.parse().ok()?))
}
