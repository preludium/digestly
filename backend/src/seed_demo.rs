//! `digestly --seed` - the test-mode/seed command (prompt.md §13). Ingests the bundled
//! `tests/fixtures/*` feeds **offline** (no network) into a throwaway DB, then builds and prints a
//! sample digest to stdout. This is a dev/CI tool run from the source checkout (the fixtures live
//! there, not in the runtime image).

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::ingest::settings::IngestSettings;
use crate::ingest::{parse, store, FeedKind};
use crate::{db, digest, seed};

/// `(fixture file, feed_url, kind, category name)` - each mapped to a seeded per-user category.
const FIXTURES: [(&str, &str, FeedKind, &str); 3] = [
    (
        "tests/fixtures/sample_rss.xml",
        "https://fixtures.example/eng/feed.xml",
        FeedKind::Rss,
        "Software Engineering",
    ),
    (
        "tests/fixtures/sample_atom.xml",
        "https://fixtures.example/ai/feed.xml",
        FeedKind::Atom,
        "AI",
    ),
    (
        "tests/fixtures/sample_jsonfeed.json",
        "https://fixtures.example/finance/feed.json",
        FeedKind::JsonFeed,
        "Finance",
    ),
];

pub async fn run() -> Result<()> {
    // Throwaway DB so this never touches production data.
    let db_path = std::path::PathBuf::from("seed-demo.db");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("seed-demo.db{suffix}"));
    }
    let pool = db::connect(&db_path).await?;
    db::migrate(&pool).await?;

    // A demo user with the standard seeded categories.
    let user_id: i64 = sqlx::query("INSERT INTO users (username, display_username, password_hash, role) VALUES ('demo', 'demo', 'x', 'user') RETURNING id")
        .fetch_one(&pool)
        .await?
        .get("id");
    seed::seed_default_categories(&pool, user_id).await?;

    let cfg = IngestSettings::default();
    let mut total = 0usize;
    for (file, feed_url, kind, category) in FIXTURES {
        let path = format!("{}/{file}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).with_context(|| format!("reading fixture {path}"))?;
        let parsed = parse::parse_feed(&bytes, feed_url, kind, &cfg, Utc::now())?;

        let feed_id: i64 = sqlx::query(
            "INSERT INTO feeds (feed_url, kind, next_fetch_at) VALUES (?, ?, datetime('now')) RETURNING id",
        )
        .bind(feed_url)
        .bind(kind.as_str())
        .fetch_one(&pool)
        .await?
        .get("id");

        let cat_id: i64 = sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = ?")
            .bind(user_id)
            .bind(category)
            .fetch_one(&pool)
            .await?
            .get("id");
        sqlx::query("INSERT INTO subscriptions (user_id, feed_id, category_id) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(feed_id)
            .bind(cat_id)
            .execute(&pool)
            .await?;

        store::apply_feed_metadata(&pool, feed_id, &parsed).await?;
        let n = store::insert_items(&pool, feed_id, &parsed.items, 0).await?;
        total += n;
        println!("ingested {n:>2} items from {file}");
    }

    // Demo convenience: stamp the ingested items as "now" so they fall inside the digest look-back
    // window (the fixtures keep their stable historical dates for the parser tests).
    sqlx::query("UPDATE items SET published_at = datetime('now')")
        .execute(&pool)
        .await?;

    // Digest without AI (fully offline).
    set(&pool, "digest.ai_enabled", "false").await?;
    let http = crate::ingest::fetch::build_client();
    digest::run_all(&pool, &http, &[0u8; 32], None).await?;

    print_digest(&pool, user_id, total).await?;

    pool.close().await;
    Ok(())
}

async fn print_digest(pool: &SqlitePool, user_id: i64, total: usize) -> Result<()> {
    let row = sqlx::query(
        "SELECT payload_json, item_count FROM digests WHERE user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        println!("\n(no digest produced)");
        return Ok(());
    };
    let payload: Value =
        serde_json::from_str(row.get::<String, _>("payload_json").as_str()).unwrap_or(Value::Null);

    println!("\n=== Sample digest (from {total} fixture items) ===");
    if let Some(cats) = payload["categories"].as_array() {
        for c in cats {
            let name = c["name"].as_str().unwrap_or("?");
            let items = c["items"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("\n## {name} ({items})");
            if let Some(list) = c["items"].as_array() {
                for it in list {
                    println!("  - {}", it["title"].as_str().unwrap_or("(untitled)"));
                }
            }
        }
    }
    if let Some(sources) = payload["sources"].as_array() {
        let names: Vec<&str> = sources.iter().filter_map(|s| s.as_str()).collect();
        println!("\nSources: {}", names.join(", "));
    }
    Ok(())
}

async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}
