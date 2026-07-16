//! Per-user seeding shared by admin bootstrap and self-registration (prompt.md §2).

use anyhow::Result;
use sqlx::SqlitePool;

/// The single default category seeded per account. `Other` is non-deletable (§2, §TODO-9).
pub const DEFAULT_CATEGORIES: [&str; 1] = ["Other"];

/// The protected catch-all category name.
pub const OTHER_CATEGORY: &str = "Other";

/// Optional starter feeds offered during onboarding (prompt.md §3, §9.11) - `(feed_url, kind,
/// category name)`. Never force-subscribed; the user opts in.
pub const STARTER_FEEDS: [(&str, &str, &str); 4] = [
    (
        "https://news.ycombinator.com/rss",
        "rss",
        "Software Engineering",
    ),
    (
        "https://www.reddit.com/r/programming/.rss",
        "reddit",
        "Software Engineering",
    ),
    (
        "https://www.reddit.com/r/MachineLearning/.rss",
        "reddit",
        "AI",
    ),
    (
        "https://www.reddit.com/r/softwareengineering/.rss",
        "reddit",
        "Software Engineering",
    ),
];

/// Seed the default categories for a user. Idempotent (UNIQUE(user_id, name) → INSERT OR IGNORE).
pub async fn seed_default_categories(pool: &SqlitePool, user_id: i64) -> Result<()> {
    for (position, name) in DEFAULT_CATEGORIES.iter().enumerate() {
        sqlx::query("INSERT OR IGNORE INTO categories (user_id, name, position) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(name)
            .bind(position as i64)
            .execute(pool)
            .await?;
    }
    Ok(())
}
