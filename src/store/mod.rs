use crate::*;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use sqlx::{Acquire as _, Row as _};
use std::{str::FromStr as _, time::Duration};
use stmt::*;
use tracing::trace_span;

mod stmt;

/// Store that persists data across executions.
#[derive(Clone)]
pub struct LamStore {
    conn: SqlitePool,
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
    pub async fn new(path: &str) -> LamResult<Self> {
        let options = SqliteConnectOptions::from_str(path)?
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(false)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let conn = SqlitePoolOptions::new()
            .max_connections(1) // TODO remove if not deadlocked
            .connect_with(options)
            .await?;
        Ok(Self { conn })
    }

    /// Create and migrate a database in memory. For testing purpose only.
    pub async fn new_in_memory() -> Self {
        let conn = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("cannot create in-memory database");
        let store = Self { conn };
        store
            .migrate()
            .await
            .expect("cannot migrate in-memory database");
        store
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
    pub async fn migrate(&self) -> LamResult<()> {
        let _ = trace_span!("run migrations").entered();
        let migrator = sqlx::migrate!();
        migrator.run(&self.conn).await?;
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
    /// assert_eq!(LamValue::from(true), store.get("a").unwrap());
    /// store.insert("b", &1.into());
    /// assert_eq!(LamValue::from(1), store.get("b").unwrap());
    /// store.insert("c", &"hello".into());
    /// assert_eq!(LamValue::from("hello"), store.get("c").unwrap());
    /// ```
    pub async fn insert<S: AsRef<str>>(&self, name: S, value: &LamValue) -> LamResult<()> {
        let name = name.as_ref();
        let value = rmp_serde::to_vec(&value)?;
        sqlx::query(SQL_UPSERT_STORE)
            .bind(name)
            .bind(value)
            .execute(&self.conn)
            .await?;
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
    pub async fn get<S: AsRef<str>>(&self, name: S) -> LamResult<LamValue> {
        let name = name.as_ref();
        let rows = sqlx::query(SQL_GET_VALUE_BY_NAME)
            .bind(name)
            .fetch_all(&self.conn)
            .await?;
        let value = match rows.first() {
            Some(r) => r.get::<Vec<u8>, usize>(0),
            None => rmp_serde::to_vec(&LamValue::None)?,
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
    pub async fn update<S: AsRef<str>>(
        &self,
        name: S,
        f: impl FnOnce(&mut LamValue) -> mlua::Result<()>,
        default_v: Option<LamValue>,
    ) -> LamResult<LamValue> {
        let mut conn = self.conn.acquire().await?;
        let mut tx = conn.begin().await?;

        let name = name.as_ref();

        let rows = sqlx::query(SQL_GET_VALUE_BY_NAME)
            .bind(name)
            .fetch_all(&mut *tx)
            .await?;
        let v: Vec<u8> = match rows.first() {
            Some(r) => r.get::<Vec<u8>, usize>(0),
            None => rmp_serde::to_vec(&default_v.unwrap_or(LamValue::None))?,
        };

        let mut deserialized = rmp_serde::from_slice(&v)?;
        let Ok(_) = f(&mut deserialized) else {
            // the function throws an error instead of returing a new value,
            // return the old value instead.
            return Ok(deserialized);
        };
        let serialized = rmp_serde::to_vec(&deserialized)?;

        sqlx::query(SQL_UPSERT_STORE)
            .bind(name)
            .bind(serialized)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        Ok(deserialized)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use maplit::hashmap;
    use std::io::empty;
    use test_case::test_case;

    #[test_case(vec![true.into(), 1.into(), "hello".into()].into())]
    #[test_case(hashmap! { "b" => true.into() }.into())]
    #[tokio::test]
    async fn complicated_types(value: LamValue) {
        let store = LamStore::new_in_memory().await;
        store.insert("value", &value).await.unwrap();
        assert_eq!("table: 0x0", store.get("value").await.unwrap().to_string());
    }

    #[tokio::test]
    async fn concurrency() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            return v+1
        end, 0)
        "#;
        let local = tokio::task::LocalSet::new();
        let store = LamStore::new_in_memory().await;
        for _ in 0..=1000 {
            local.spawn_local({
                let store = store.clone();
                async move {
                    let e = EvaluationBuilder::new(script, empty())
                        .with_store(store)
                        .build();
                    e.evaluate_async().await.unwrap();
                }
            });
        }
        local.await;
        assert_eq!(LamValue::from(1001), store.get("a").await.unwrap());
    }

    #[tokio::test]
    async fn get_set() {
        let script = r#"
        local m = require('@lam')
        local a = m:get('a')
        m:set('a', 4.56)
        return a
        "#;

        let store = LamStore::new_in_memory().await;
        store.insert("a", &1.23.into()).await.unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate_async().await.unwrap();
        assert_eq!(LamValue::from(1.23), res.result);
        assert_eq!(LamValue::from(4.56), store.get("a").await.unwrap());
    }

    #[tokio::test]
    async fn migrate() {
        let store = LamStore::new_in_memory().await;
        store.migrate().await.unwrap(); // duplicated
    }

    #[test_case("nil", LamValue::None)]
    #[test_case("bt", true.into())]
    #[test_case("bf", false.into())]
    #[test_case("ni", 1.into())]
    #[test_case("nf", 1.23.into())]
    #[test_case("s", "hello".into())]
    #[tokio::test]
    async fn primitive_types(key: &'static str, value: LamValue) {
        let store = LamStore::new_in_memory().await;
        store.insert(key, &value).await.unwrap();
        assert_eq!(value, store.get(key).await.unwrap());
    }

    #[tokio::test]
    async fn reuse() {
        let script = r#"
        local m = require('@lam')
        local a = m:get('a')
        m:set('a', a+1)
        return a
        "#;

        let store = LamStore::new_in_memory().await;
        store.insert("a", &1.into()).await.unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        {
            let res = e.evaluate_async().await.unwrap();
            assert_eq!(LamValue::from(1), res.result);
            assert_eq!(LamValue::from(2), store.get("a").await.unwrap());
        }

        {
            let res = e.evaluate_async().await.unwrap();
            assert_eq!(LamValue::from(2), res.result);
            assert_eq!(LamValue::from(3), store.get("a").await.unwrap());
        }
    }

    #[tokio::test]
    async fn update_without_default_value() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            return v+1
        end)
        "#;

        let store = LamStore::new_in_memory().await;
        store.insert("a", &1.into()).await.unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate_async().await.unwrap();
        assert_eq!(LamValue::from(2), res.result);
        assert_eq!(LamValue::from(2), store.get("a").await.unwrap());
    }

    #[tokio::test]
    async fn rollback_when_error() {
        let script = r#"
        return require('@lam'):update('a', function(v)
            if v == 1 then
                error('something went wrong')
            else
                return v+1
            end
        end, 0)
        "#;

        let store = LamStore::new_in_memory().await;
        store.insert("a", &1.into()).await.unwrap();

        let e = EvaluationBuilder::new(script, empty())
            .with_store(store.clone())
            .build();

        let res = e.evaluate_async().await.unwrap();
        assert_eq!(LamValue::from(1), res.result);
        assert_eq!(LamValue::from(1), store.get("a").await.unwrap());
    }
}
