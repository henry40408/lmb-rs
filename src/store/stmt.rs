pub(crate) const SQL_DELETE_VALUE_BY_NAME: &str = "DELETE FROM store WHERE name = ?1";

pub(crate) const SQL_GET_ALL_VALUES: &str = "SELECT name, type, created_at, updated_at FROM store;";

pub(crate) const SQL_GET_VALUE_BY_NAME: &str = "SELECT value, type FROM store WHERE name = ?1";

pub(crate) const SQL_UPSERT_STORE: &str = r#"
    INSERT INTO store (name, value, size, type) VALUES (?1, ?2, ?3, ?4)
    ON CONFLICT(name) DO UPDATE SET value = ?2, size = ?3, type = ?4, updated_at = CURRENT_TIMESTAMP
"#;
