use sqlx::{Row, SqlitePool};

pub async fn get_str(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<String, _>("value"))
}

pub async fn get_int(pool: &SqlitePool, key: &str, default: i64) -> i64 {
    get_str(pool, key)
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub async fn get_bool(pool: &SqlitePool, key: &str, default: bool) -> bool {
    get_str(pool, key)
        .await
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn get_str_reads_existing_value_and_missing_as_none() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO app_settings (key, value) VALUES ('example.key', 'value')")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            get_str(&pool, "example.key").await.as_deref(),
            Some("value")
        );
        assert_eq!(get_str(&pool, "missing.key").await, None);
    }

    #[tokio::test]
    async fn get_int_preserves_parse_and_default_semantics() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES
             ('int.valid', '42'),
             ('int.invalid', 'nope')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(get_int(&pool, "int.valid", 7).await, 42);
        assert_eq!(get_int(&pool, "int.invalid", 7).await, 7);
        assert_eq!(get_int(&pool, "int.missing", 7).await, 7);
    }

    #[tokio::test]
    async fn get_bool_preserves_true_one_and_falsey_semantics() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES
             ('bool.true', 'true'),
             ('bool.one', '1'),
             ('bool.false', 'false'),
             ('bool.other', 'yes')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(get_bool(&pool, "bool.true", false).await);
        assert!(get_bool(&pool, "bool.one", false).await);
        assert!(!get_bool(&pool, "bool.false", true).await);
        assert!(!get_bool(&pool, "bool.other", true).await);
        assert!(get_bool(&pool, "bool.missing", true).await);
    }
}
