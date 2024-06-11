use chrono::{DateTime, Utc};
use get_size::GetSize as _;
use parking_lot::Mutex;
use rusqlite::Connection;
use rusqlite_migration::SchemaVersion;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use stmt::*;
use tracing::{debug, trace, trace_span};

use crate::{Result, MIGRATIONS};

mod stmt;

/// Store options for command line.
#[derive(Default)]
pub struct StoreOptions {
    /// Store path.
    pub store_path: Option<PathBuf>,
    /// Run migrations.
    pub run_migrations: bool,
}

/// Store that persists data across executions.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

fn type_hint(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

impl Store {
    /// Create a new store with path on the filesystem.
    ///
    /// ```rust
    /// # use assert_fs::NamedTempFile;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store_file = NamedTempFile::new("db.sqlite3")?;
    /// Store::new(store_file.path())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(path: &Path) -> Result<Self> {
        debug!(?path, "open store");
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Perform migration on the database. Migrations should be idempotent. If version is omitted,
    /// database will be migrated to the latest. If version is 0, all migrations will be reverted.
    ///
    /// ```rust
    /// # use assert_fs::NamedTempFile;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store_file = NamedTempFile::new("db.sqlite3")?;
    /// let store = Store::new(store_file.path())?;
    /// store.migrate(None)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn migrate(&self, version: Option<usize>) -> Result<()> {
        let mut conn = self.conn.lock();
        if let Some(version) = version {
            let _s = trace_span!("migrate_to_version", version).entered();
            MIGRATIONS.to_version(&mut conn, version)?;
        } else {
            let _s = trace_span!("migrate_to_latest").entered();
            MIGRATIONS.to_latest(&mut conn)?;
        }
        Ok(())
    }

    /// Return current version of migrations.
    pub fn current_version(&self) -> Result<SchemaVersion> {
        let conn = self.conn.lock();
        let version = MIGRATIONS.current_version(&conn)?;
        Ok(version)
    }

    /// Delete value by name.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// assert_eq!(json!(null), store.get("a")?);
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// store.delete("a");
    /// assert_eq!(json!(null), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete<S>(&self, name: S) -> Result<usize>
    where
        S: AsRef<str>,
    {
        let conn = self.conn.lock();
        let affected = conn.execute(SQL_DELETE_VALUE_BY_NAME, (name.as_ref(),))?;
        Ok(affected)
    }

    /// Get value from the store. A `nil` will be returned to Lua virtual machine
    /// when the value is absent.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// assert_eq!(json!(null), store.get("a")?);
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn get<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        let conn = self.conn.lock();

        let name = name.as_ref();

        let mut cached_stmt = conn.prepare_cached(SQL_GET_VALUE_BY_NAME)?;
        let _s = trace_span!("store_get", name).entered();
        let res = cached_stmt.query_row((name,), |row| {
            let value: Vec<u8> = row.get_unwrap("value");
            let type_hint: String = row.get_unwrap("type_hint");
            Ok((value, type_hint))
        });
        let value: Vec<u8> = match res {
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                trace!("no_value");
                return Ok(Value::Null);
            }
            Err(e) => return Err(e.into()),
            Ok((v, type_hint)) => {
                trace!(type_hint, "value");
                v
            }
        };

        Ok(rmp_serde::from_slice::<Value>(&value)?)
    }

    /// List values.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &true.into())?;
    /// let values = store.list()?;
    /// assert_eq!(1, values.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn list(&self) -> Result<Vec<StoreValueMetadata>> {
        let conn = self.conn.lock();
        let mut cached_stmt = conn.prepare_cached(SQL_GET_ALL_VALUES)?;
        let mut rows = cached_stmt.query([])?;
        let mut res = vec![];
        while let Some(row) = rows.next()? {
            let name: String = row.get_unwrap("name");
            let type_hint: String = row.get_unwrap("type_hint");
            let size: usize = row.get_unwrap("size");
            let created_at: DateTime<Utc> = row.get_unwrap("created_at");
            let updated_at: DateTime<Utc> = row.get_unwrap("updated_at");
            res.push(StoreValueMetadata {
                name,
                size,
                type_hint,
                created_at,
                updated_at,
            });
        }
        Ok(res)
    }

    /// Put (insert or update) the value into the store.
    ///
    /// The key distinction between this function and [`Store::update`] is
    /// that this function unconditionally puts with the provided value.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// store.put("b", &1.into());
    /// assert_eq!(json!(1), store.get("b")?);
    /// store.put("c", &"hello".into());
    /// assert_eq!(json!("hello"), store.get("c")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn put<S: AsRef<str>>(&self, name: S, value: &Value) -> Result<usize> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let size = value.get_size();
        let type_hint = type_hint(value);
        let value = rmp_serde::to_vec(&value)?;

        let mut cached_stmt = conn.prepare_cached(SQL_UPSERT_STORE)?;
        let _s = trace_span!("store_insert", name, type_hint).entered();
        let affected = cached_stmt.execute((name, value, size, type_hint))?;

        Ok(affected)
    }

    /// Insert or update the value into the store.
    ///
    /// Unlike [`Store::put`], this function accepts a closure and only mutates the value in the store
    /// when the closure returns a new value. If the closure results in an error,
    /// the value in the store remains unchanged.
    ///
    /// This function also takes a default value.
    ///
    /// # Successfully update the value
    ///
    /// ```rust
    /// # use serde_json::{json, Value};
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// let updated = store.update("b", |old| {
    ///     if let Value::Number(_) = old {
    ///         let n = old.as_i64().ok_or(mlua::Error::runtime("n is required"))?;
    ///         *old = json!(n + 1);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(json!(2), updated?);
    /// assert_eq!(json!(2), store.get("b")?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Do nothing when an error is returned
    ///
    /// ```rust
    /// # use serde_json::{json, Value};
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &1.into());
    /// let updated = store.update("a", |old| {
    ///     if let Value::Number(_) = old {
    ///        let n = old.as_i64().ok_or(mlua::Error::runtime("n is required"))?;
    ///        if n == 1 {
    ///            return Err(mlua::Error::runtime("something went wrong"));
    ///        }
    ///        *old = json!(n + 1);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(json!(1), updated?);
    /// assert_eq!(json!(1), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn update<S: AsRef<str>>(
        &self,
        name: S,
        f: impl FnOnce(&mut Value) -> mlua::Result<()>,
        default_v: Option<Value>,
    ) -> Result<Value> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let name = name.as_ref();

        let _s = trace_span!("store_update", name).entered();
        let value: Vec<u8> = {
            let mut cached_stmt = tx.prepare_cached(SQL_GET_VALUE_BY_NAME)?;
            match cached_stmt.query_row((name,), |row| row.get(0)) {
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    trace!("default_value");
                    rmp_serde::to_vec(default_v.as_ref().unwrap_or(&Value::Null))?
                }
                Err(e) => return Err(e.into()),
                Ok(v) => {
                    trace!("value");
                    v
                }
            }
        };

        let mut value: Value = rmp_serde::from_slice(&value)?;
        {
            let _s = trace_span!("call_function").entered();
            let Ok(_) = f(&mut value) else {
                // the function throws an error instead of returing a new value,
                // return the old value instead.
                trace!("failed");
                return Ok(value);
            };
        }
        let size = (&value).get_size();
        let type_hint = type_hint(&value);
        {
            let value = rmp_serde::to_vec(&value)?;
            let mut cached_stmt = tx.prepare_cached(SQL_UPSERT_STORE)?;
            cached_stmt.execute((name, value, size, type_hint))?;
        }
        tx.commit()?;
        trace!(type_hint, "updated");

        Ok(value)
    }
}

/// Value metadata. The value itself is intentionally not included.
pub struct StoreValueMetadata {
    /// Name.
    pub name: String,
    /// Size in bytes.
    pub size: usize,
    /// Type.
    pub type_hint: String,
    /// Timestamp indicating when the value was created in UTC timezone.
    pub created_at: DateTime<Utc>,
    /// Timestamp indicating when the value was updated in UTC timezone.
    pub updated_at: DateTime<Utc>,
}

impl Default for Store {
    /// Open and initialize a SQLite database in memory.
    fn default() -> Self {
        debug!("open store in memory");
        let conn = Connection::open_in_memory().expect("failed to open SQLite database in memory");
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store
            .migrate(None)
            .expect("failed to migrate SQLite database in memory");
        store
    }
}

#[cfg(test)]
mod tests {
    use assert_fs::NamedTempFile;
    use serde_json::{json, Value};
    use std::{io::empty, thread};
    use test_case::test_case;

    use crate::{EvaluationBuilder, Store};

    #[test]
    fn concurrency() {
        let script = r#"
        return require('@lmb'):update('a', function(v)
            return v+1
        end, 0)
        "#;

        let store = Store::default();

        let mut threads = vec![];
        for _ in 0..=1000 {
            let store = store.clone();
            threads.push(thread::spawn(move || {
                let e = EvaluationBuilder::new(script, empty()).store(store).build();
                e.evaluate().unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(json!(1001), store.get("a").unwrap());
    }

    #[test]
    fn get_put() {
        let script = r#"
        local m = require('@lmb')
        local a = m:get('a')
        assert(not m:get('b'))
        m:put('a', 4.56)
        return a
        "#;

        let store = Store::default();
        store.put("a", &1.23.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(json!(1.23), res.payload);
        assert_eq!(json!(4.56), store.get("a").unwrap());
        assert_eq!(json!(null), store.get("b").unwrap());
    }

    #[test]
    fn migrate() {
        let store = Store::default();
        store.migrate(None).unwrap(); // duplicated
        store.current_version().unwrap();
        store.migrate(Some(0)).unwrap();
    }

    #[test]
    fn new_store() {
        let store_file = NamedTempFile::new("db.sqlite3").unwrap();
        let store = Store::new(store_file.path()).unwrap();
        store.migrate(None).unwrap();
    }

    #[test_case("nil", json!(null))]
    #[test_case("bt", json!(true))]
    #[test_case("bf", json!(false))]
    #[test_case("ni", json!(1))]
    #[test_case("nf", json!(1.23))]
    #[test_case("s", json!("hello"))]
    fn primitive_types(key: &'static str, value: Value) {
        let store = Store::default();
        store.put(key, &value).unwrap();
        assert_eq!(value, store.get(key).unwrap());
    }

    #[test]
    fn reuse() {
        let script = r#"
        local m = require('@lmb')
        local a = m:get('a')
        m:put('a', a+1)
        return a
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .store(store.clone())
            .build();

        {
            let res = e.evaluate().unwrap();
            assert_eq!(json!(1), res.payload);
            assert_eq!(json!(2), store.get("a").unwrap());
        }

        {
            let res = e.evaluate().unwrap();
            assert_eq!(json!(2), res.payload);
            assert_eq!(json!(3), store.get("a").unwrap());
        }
    }

    #[test]
    fn update_without_default_value() {
        let script = r#"
        return require('@lmb'):update('a', function(v)
            return v+1
        end)
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(json!(2), res.payload);
        assert_eq!(json!(2), store.get("a").unwrap());
    }

    #[test_log::test]
    fn rollback_when_error() {
        let script = r#"
        return require('@lmb'):update('a', function(v)
            if v == 1 then
                error('something went wrong')
            else
                return v+1
            end
        end, 0)
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(json!(1), res.payload);
        assert_eq!(json!(1), store.get("a").unwrap());
    }
}
