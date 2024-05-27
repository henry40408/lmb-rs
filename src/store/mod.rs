use chrono::{DateTime, Utc};
use get_size::GetSize;
use parking_lot::Mutex;
use rusqlite::Connection;
use rusqlite_migration::SchemaVersion;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use stmt::*;
use tracing::{debug, trace, trace_span};

use crate::{LamResult, LamValue, MIGRATIONS};

mod stmt;

/// Store options for command line
#[derive(Default)]
pub struct StoreOptions {
    /// Store path
    pub store_path: Option<PathBuf>,
    /// Run migrations
    pub run_migrations: bool,
}

/// Store that persists data across executions.
#[derive(Clone)]
pub struct LamStore {
    conn: Arc<Mutex<Connection>>,
}

impl LamStore {
    /// Create a new store with path on the filesystem.
    ///
    /// ```rust
    /// # use tempdir::TempDir;
    /// use lam::*;
    /// let dir = TempDir::new("temp").unwrap();
    /// let path = dir.path().join("db.sqlite3");
    /// let _ = LamStore::new(&path);
    /// ```
    pub fn new(path: &Path) -> LamResult<Self> {
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

    /// Perform migration on the database. Migrations should be idempotent.
    ///
    /// ```rust
    /// # use tempdir::TempDir;
    /// use lam::*;
    /// let dir = TempDir::new("temp").unwrap();
    /// let path = dir.path().join("db.sqlite3");
    /// let store = LamStore::new(&path).unwrap();
    /// store.migrate(None).unwrap();
    /// ```
    pub fn migrate(&self, version: Option<usize>) -> LamResult<()> {
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

    /// Return current version of migrations
    pub fn current_version(&self) -> LamResult<SchemaVersion> {
        let conn = self.conn.lock();
        let version = MIGRATIONS.current_version(&conn)?;
        Ok(version)
    }

    /// Insert or update the value into the store.
    ///
    /// The key distinction between this function and [`LamStore::update`] is
    /// that this function unconditionally inserts or updates with the provided value.
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// store.insert("a", &true.into());
    /// assert_eq!(LamValue::from(true), store.get("a").unwrap());
    /// store.insert("b", &1.into());
    /// assert_eq!(LamValue::from(1), store.get("b").unwrap());
    /// store.insert("c", &"hello".into());
    /// assert_eq!(LamValue::from("hello"), store.get("c").unwrap());
    /// ```
    pub fn insert<S: AsRef<str>>(&self, name: S, value: &LamValue) -> LamResult<()> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let size = value.get_size();
        let type_hint = value.type_hint();
        let value = rmp_serde::to_vec(&value)?;

        let mut cached_stmt = conn.prepare_cached(SQL_UPSERT_STORE)?;
        let _s = trace_span!("store_insert", name, type_hint).entered();
        cached_stmt.execute((name, value, size, type_hint))?;

        Ok(())
    }

    /// Get value from the store. A `nil` will be returned to Lua virtual machine
    /// when the value is absent.
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// assert_eq!(LamValue::None, store.get("a").unwrap());
    /// store.insert("a", &true.into());
    /// assert_eq!(LamValue::from(true), store.get("a").unwrap());
    /// ```
    pub fn get<S: AsRef<str>>(&self, name: S) -> LamResult<LamValue> {
        let conn = self.conn.lock();

        let name = name.as_ref();

        let mut cached_stmt = conn.prepare_cached(SQL_GET_VALUE_BY_NAME)?;
        let _s = trace_span!("store_get", name).entered();
        let res = cached_stmt.query_row((name,), |row| {
            let value: Vec<u8> = row.get("value")?;
            let type_hint: String = row.get("type")?;
            Ok((value, type_hint))
        });
        let value: Vec<u8> = match res {
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                trace!("no_value");
                return Ok(LamValue::None);
            }
            Err(e) => return Err(e.into()),
            Ok((v, type_hint)) => {
                trace!(type_hint, "value");
                v
            }
        };

        Ok(rmp_serde::from_slice::<LamValue>(&value)?)
    }

    /// Insert or update the value into the store.
    ///
    /// Unlike [`LamStore::insert`], this function accepts a closure and only mutates the value in the store
    /// when the closure returns a new value. If the closure results in an error,
    /// the value in the store remains unchanged.
    ///
    /// This function also takes a default value.
    ///
    /// # Successfully update the value
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// let x = store.update("b", |old| {
    ///     if let LamValue::Integer(n) = old {
    ///         *old = LamValue::from(*n + 1);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(LamValue::from(2), x.unwrap());
    /// assert_eq!(LamValue::from(2), store.get("b").unwrap());
    /// ```
    ///
    /// # Do nothing when an error is returned
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// store.insert("a", &1.into());
    /// let x = store.update("a", |old| {
    ///     if let LamValue::Integer(n) = old {
    ///        if *n == 1 {
    ///            return Err(mlua::Error::runtime("something went wrong"));
    ///        }
    ///        *old = LamValue::from(*n + 1);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(LamValue::from(1), x.unwrap());
    /// assert_eq!(LamValue::from(1), store.get("a").unwrap());
    /// ```
    pub fn update<S: AsRef<str>>(
        &self,
        name: S,
        f: impl FnOnce(&mut LamValue) -> mlua::Result<()>,
        default_v: Option<LamValue>,
    ) -> LamResult<LamValue> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let name = name.as_ref();

        let _s = trace_span!("store_update", name).entered();
        let value: Vec<u8> = {
            let mut cached_stmt = tx.prepare_cached(SQL_GET_VALUE_BY_NAME)?;
            match cached_stmt.query_row((name,), |row| row.get(0)) {
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    trace!("default_value");
                    rmp_serde::to_vec(default_v.as_ref().unwrap_or(&LamValue::None))?
                }
                Err(e) => return Err(e.into()),
                Ok(v) => {
                    trace!("value");
                    v
                }
            }
        };

        let mut value = rmp_serde::from_slice(&value)?;
        {
            let _s = trace_span!("call_function").entered();
            let Ok(_) = f(&mut value) else {
                // the function throws an error instead of returing a new value,
                // return the old value instead.
                trace!("failed");
                return Ok(value);
            };
        }
        let size = value.get_size();
        let type_hint = value.type_hint();
        {
            let value = rmp_serde::to_vec(&value)?;
            let mut cached_stmt = tx.prepare_cached(SQL_UPSERT_STORE)?;
            cached_stmt.execute((name, value, size, type_hint))?;
        }
        tx.commit()?;
        trace!(type_hint, "updated");

        Ok(value)
    }

    /// List values
    pub fn list(&self) -> LamResult<Vec<LamValueMetadata>> {
        let conn = self.conn.lock();
        let mut cached_stmt = conn.prepare_cached(SQL_GET_ALL_VALUES)?;
        let mut rows = cached_stmt.query([])?;
        let mut res = vec![];
        while let Some(row) = rows.next()? {
            let name: String = row.get("name")?;
            let type_hint: String = row.get("type")?;
            let created_at: DateTime<Utc> = row.get("created_at")?;
            let updated_at: DateTime<Utc> = row.get("updated_at")?;
            res.push(LamValueMetadata {
                name,
                type_hint,
                created_at,
                updated_at,
            });
        }
        Ok(res)
    }
}

/// Value meatadata
pub struct LamValueMetadata {
    /// Name
    pub name: String,
    /// Type
    pub type_hint: String,
    /// Created at
    pub created_at: DateTime<Utc>,
    /// Updated at
    pub updated_at: DateTime<Utc>,
}

impl Default for LamStore {
    fn default() -> Self {
        debug!("open store in memory");
        let conn = Connection::open_in_memory().expect("failed to open sqlite in memory");
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store
            .migrate(None)
            .expect("failed to migrate database in memory");
        store
    }
}

#[cfg(test)]
mod tests {
    use maplit::hashmap;
    use std::{io::empty, thread};
    use tempfile::NamedTempFile;
    use test_case::test_case;

    use crate::{EvaluationBuilder, LamStore, LamValue};

    #[test_case(vec![true.into(), 1.into(), "hello".into()].into())]
    #[test_case(hashmap! { "b" => true.into() }.into())]
    fn complicated_types(value: LamValue) {
        let store = LamStore::default();
        store.insert("value", &value).unwrap();
        let actual = store.get("value").unwrap().to_string();
        assert!(actual.starts_with("table: 0x"));
    }

    #[test]
    fn concurrency() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            return v+1
        end, 0)
        "#;

        let store = LamStore::default();

        let mut threads = vec![];
        for _ in 0..=1000 {
            let store = store.clone();
            threads.push(thread::spawn(move || {
                let e = EvaluationBuilder::new(script, empty())
                    .with_store(store)
                    .build();
                e.evaluate().unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(LamValue::from(1001), store.get("a").unwrap());
    }

    #[test]
    fn get_set() {
        let script = r#"
        local m = require('@lam')
        local a = m:get('a')
        m:set('a', 4.56)
        return a
        "#;

        let store = LamStore::default();
        store.insert("a", &1.23.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(1.23), res.payload);
        assert_eq!(LamValue::from(4.56), store.get("a").unwrap());
        assert_eq!(LamValue::None, store.get("b").unwrap());
    }

    #[test]
    fn migrate() {
        let store = LamStore::default();
        store.migrate(None).unwrap(); // duplicated
        store.current_version().unwrap();
        store.migrate(Some(0)).unwrap();
    }

    #[test]
    fn new_store() {
        let store_path = NamedTempFile::new().unwrap();
        let store = LamStore::new(store_path.path()).unwrap();
        store.migrate(None).unwrap();
    }

    #[test_case("nil", LamValue::None)]
    #[test_case("bt", true.into())]
    #[test_case("bf", false.into())]
    #[test_case("ni", 1.into())]
    #[test_case("nf", 1.23.into())]
    #[test_case("s", "hello".into())]
    fn primitive_types(key: &'static str, value: LamValue) {
        let store = LamStore::default();
        store.insert(key, &value).unwrap();
        assert_eq!(value, store.get(key).unwrap());
    }

    #[test]
    fn reuse() {
        let script = r#"
        local m = require('@lam')
        local a = m:get('a')
        m:set('a', a+1)
        return a
        "#;

        let store = LamStore::default();
        store.insert("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        {
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::from(1), res.payload);
            assert_eq!(LamValue::from(2), store.get("a").unwrap());
        }

        {
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::from(2), res.payload);
            assert_eq!(LamValue::from(3), store.get("a").unwrap());
        }
    }

    #[test]
    fn update_without_default_value() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            return v+1
        end)
        "#;

        let store = LamStore::default();
        store.insert("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(2), res.payload);
        assert_eq!(LamValue::from(2), store.get("a").unwrap());
    }

    #[test_log::test]
    fn rollback_when_error() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            if v == 1 then
                error('something went wrong')
            else
                return v+1
            end
        end, 0)
        "#;

        let store = LamStore::default();
        store.insert("a", &1.into()).unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(1), res.payload);
        assert_eq!(LamValue::from(1), store.get("a").unwrap());
    }
}
