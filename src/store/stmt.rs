pub(crate) const SQL_GET_VALUE_BY_NAME: &str = r#"SELECT value FROM store WHERE name = ?1"#;

pub(crate) const SQL_UPSERT_STORE: &str = r#"
    INSERT INTO store (name, value) VALUES (?1, ?2)
    ON CONFLICT(name) DO UPDATE SET value = ?2
"#;
