pub(crate) const SQL_DELETE_VALUE_BY_NAME: &str = "DELETE FROM store WHERE name = ?1";

pub(crate) const SQL_GET_ALL_VALUES: &str = "
    SELECT name, size, type_hint, created_at, updated_at FROM store
";

#[deprecated]
pub(crate) const SQL_GET_VALUES_BY_NAME: &str =
    "SELECT value, type_hint FROM store WHERE name = ?1";

pub(crate) const SQL_UPSERT_STORE: &str = r#"
    INSERT INTO store (name, value, size, type_hint) VALUES (?1, ?2, ?3, ?4)
    ON CONFLICT(name) DO UPDATE SET value = ?2, size = ?3, type_hint = ?4, updated_at = CURRENT_TIMESTAMP
"#;

pub(crate) fn build_sql_get_values_by_name(count: usize) -> String {
    format!(
        "SELECT value, type_hint FROM store WHERE name IN ({})",
        repeat_vars(count)
    )
}

pub(crate) fn repeat_vars(count: usize) -> String {
    assert!(count > 0, "count should be greater than zero");
    vec!["?"; count].join(",")
}

#[cfg(test)]
mod tests {
    use super::repeat_vars;

    #[test]
    #[should_panic]
    fn zero_repeat_vars() {
        repeat_vars(0);
    }

    #[test]
    fn non_zero_repear_vars() {
        repeat_vars(1);
        repeat_vars(2);
        repeat_vars(100);
    }
}
