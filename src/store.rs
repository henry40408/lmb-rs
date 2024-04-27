use crate::*;
use include_dir::{include_dir, Dir};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::{path::Path, sync::Arc};
use tracing::{debug, trace_span};

static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

const FIND_SQL: &str = r#"SELECT value FROM store WHERE name = ?1"#;
const UPDATE_SQL: &str = r#"INSERT INTO store (name, value) VALUES (?1, ?2)
    ON CONFLICT(name) DO UPDATE SET value = ?2"#;

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
    /// store.migrate().unwrap();
    /// ```
    pub fn migrate(&self) -> LamResult<()> {
        let _ = trace_span!("run migrations").entered();
        let conn = self.conn.lock();
        for e in MIGRATIONS_DIR.entries() {
            let path = e.path();
            debug!(?path, "open migration");
            let sql = e
                .as_file()
                .expect("invalid file")
                .contents_utf8()
                .expect("invalid contents");
            let _ = trace_span!("run migration SQL", ?sql).entered();
            debug!(?sql, "run migration SQL");
            conn.execute(sql, ())?;
        }
        Ok(())
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
    /// assert_eq!(LamValue::Boolean(true), store.get("a").unwrap());
    /// store.insert("b", &1.into());
    /// assert_eq!(LamValue::Number(1f64), store.get("b").unwrap());
    /// store.insert("c", &"hello".into());
    /// assert_eq!(LamValue::String("hello".into()), store.get("c").unwrap());
    /// ```
    pub fn insert<S: AsRef<str>>(&self, name: S, value: &LamValue) -> LamResult<()> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let value = rmp_serde::to_vec(&value)?;
        conn.execute(UPDATE_SQL, (name, value))?;

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
    /// assert_eq!(LamValue::Boolean(true), store.get("a").unwrap());
    /// ```
    pub fn get<S: AsRef<str>>(&self, name: S) -> LamResult<LamValue> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let v: Vec<u8> = match conn.query_row(FIND_SQL, (name,), |row| row.get(0)) {
            Err(_) => return Ok(LamValue::None),
            Ok(v) => v,
        };

        Ok(rmp_serde::from_slice::<LamValue>(&v)?)
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
    ///     if let LamValue::Number(n) = old {
    ///         *old = LamValue::Number(*n + 1f64);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(LamValue::Number(2f64), x.unwrap());
    /// assert_eq!(LamValue::Number(2f64), store.get("b").unwrap());
    /// ```
    ///
    /// # Do nothing when an error is returned
    ///
    /// ```rust
    /// use lam::*;
    /// let store = LamStore::default();
    /// store.insert("a", &1.into());
    /// let x = store.update("a", |old| {
    ///     if let LamValue::Number(n) = old {
    ///        if *n == 1f64 {
    ///            return Err(mlua::Error::runtime("something went wrong"));
    ///        }
    ///        *old = LamValue::Number(*n + 1f64);
    ///     }
    ///     Ok(())
    /// }, Some(1.into()));
    /// assert_eq!(LamValue::Number(1f64), x.unwrap());
    /// assert_eq!(LamValue::Number(1f64), store.get("a").unwrap());
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

        let v: Vec<u8> = match tx.query_row(FIND_SQL, (name,), |row| row.get(0)) {
            Err(_) => rmp_serde::to_vec(&default_v.unwrap_or(LamValue::None))?,
            Ok(v) => v,
        };

        let mut deserialized = rmp_serde::from_slice(&v)?;
        let Ok(_) = f(&mut deserialized) else {
            // the function throws an error instead of returing a new value,
            // return the old value instead.
            return Ok(deserialized);
        };
        let serialized = rmp_serde::to_vec(&deserialized)?;

        tx.execute(UPDATE_SQL, (name, serialized))?;
        tx.commit()?;

        Ok(deserialized)
    }
}

impl Default for LamStore {
    fn default() -> Self {
        let conn = Connection::open_in_memory().expect("failed to open sqlite in memory");
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store
            .migrate()
            .expect("failed to migrate database in memory");
        store
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use maplit::hashmap;
    use std::{io::empty, thread};
    use test_case::test_case;

    #[test_case(vec![true.into(), 1f64.into(), "hello".into()].into())]
    #[test_case(hashmap! { "b".into() => true.into() }.into())]
    fn complicated_types(value: LamValue) {
        let store = LamStore::default();
        store.insert("value", &value).unwrap();
        assert_eq!("table: 0x0", store.get("value").unwrap().to_string());
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
                let e = EvalBuilder::new(script.into(), empty())
                    .with_store(store)
                    .build();
                e.evaluate().unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(LamValue::Number(1001f64), store.get("a").unwrap());
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

        let e = EvalBuilder::new(script.into(), empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::Number(1.23), res.result);
        assert_eq!(LamValue::Number(4.56), store.get("a").unwrap());
    }

    #[test]
    fn migrate() {
        let store = LamStore::default();
        store.migrate().unwrap(); // duplicated
    }

    #[test_case("nil", LamValue::None)]
    #[test_case("bt", true.into())]
    #[test_case("bf", false.into())]
    #[test_case("ni", 1f64.into())]
    #[test_case("nf", 1.23f64.into())]
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
        store.insert("a", &LamValue::Number(1f64)).unwrap();

        let e = EvalBuilder::new(script.into(), empty())
            .with_store(store.clone())
            .build();

        {
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::Number(1f64), res.result);
            assert_eq!(LamValue::Number(2f64), store.get("a").unwrap());
        }

        {
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::Number(2f64), res.result);
            assert_eq!(LamValue::Number(3f64), store.get("a").unwrap());
        }
    }

    #[test]
    fn update_without_default_value() {
        let script = r#"return require('@lam'):update('a', function(v)
            return v+1
        end)"#;

        let store = LamStore::default();
        store.insert("a", &LamValue::Number(1f64)).unwrap();

        let e = EvalBuilder::new(script.into(), empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::Number(2f64), res.result);
        assert_eq!(LamValue::Number(2f64), store.get("a").unwrap());
    }

    #[test_log::test]
    fn rollback_when_error() {
        let script = r#"return require('@lam'):update('a', function(v)
            if v == 1 then
                error('something went wrong')
            else
                return v+1
            end
        end, 0)"#;

        let store = LamStore::default();
        store.insert("a", &LamValue::Number(1f64)).unwrap();

        let e = EvalBuilder::new(script.into(), empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::Number(1f64), res.result);
        assert_eq!(LamValue::Number(1f64), store.get("a").unwrap());
    }
}
