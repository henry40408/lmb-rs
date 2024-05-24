pub(crate) const SQL_GET_VALUE_BY_NAME: &str = "SELECT value, type FROM store WHERE name = ?1";

pub(crate) const SQL_UPSERT_STORE: &str = r#"
    INSERT INTO store (name, value, size, type) VALUES (?1, ?2, ?3, ?4)
    ON CONFLICT(name) DO UPDATE SET value = ?2, size = ?3, type = ?4, updated_at = strftime('%s', 'now')
"#;
