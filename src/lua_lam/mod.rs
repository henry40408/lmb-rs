use mlua::prelude::*;
use std::io::BufRead;

use crate::{LamInput, LamResult, LamState, LamStateKey, LamStore, LamValue};

use crypto::*;
use http::*;
use json::*;
use read::*;

mod crypto;
mod http;
mod json;
mod read;

// ref: https://www.lua.org/pil/8.1.html
const K_LOADED: &str = "_LOADED";

/// Interface of Lam between Lua and Rust.
pub struct LuaLam<R>
where
    R: BufRead,
{
    input: LamInput<R>,
    state: Option<LamState>,
    store: Option<LamStore>,
}

impl<R> LuaLam<R>
where
    for<'lua> R: 'lua + BufRead + Send,
{
    /// Create a new instance of interface with input [`LamInput`] and store [`LamStore`].
    ///
    /// <div class="warning">Export for benchmarking, but end-user should not directly use it.</div>
    ///
    /// ```rust
    /// # use std::{io::{Cursor, BufReader}, sync::Arc};
    /// # use parking_lot::Mutex;
    /// use lam::*;
    /// let input = Arc::new(Mutex::new(BufReader::new(Cursor::new("0"))));
    /// let store = LamStore::default();
    /// let _ = LuaLam::new(input, Some(store), None);
    /// ```
    pub fn new(input: LamInput<R>, store: Option<LamStore>, state: Option<LamState>) -> Self {
        Self {
            input,
            state,
            store,
        }
    }

    /// Register the interface to a Lua virtual machine.
    ///
    /// ```rust
    /// # use std::{io::{Cursor, BufReader}, sync::Arc};
    /// # use mlua::prelude::*;
    /// # use parking_lot::Mutex;
    /// use lam::*;
    /// let vm = Lua::new();
    /// let input = Arc::new(Mutex::new(BufReader::new(Cursor::new("0"))));
    /// let store = LamStore::default();
    /// let _ = LuaLam::register(&vm, input, Some(store), None);
    /// ```
    pub fn register(
        vm: &Lua,
        input: LamInput<R>,
        store: Option<LamStore>,
        state: Option<LamState>,
    ) -> LamResult<()> {
        let loaded = vm.named_registry_value::<LuaTable<'_>>(K_LOADED)?;
        loaded.set("@lam", Self::new(input, store, state))?;
        loaded.set("@lam/crypto", LuaLamCrypto {})?;
        loaded.set("@lam/http", LuaLamHTTP {})?;
        loaded.set("@lam/json", LuaLamJSON {})?;
        vm.set_named_registry_value(K_LOADED, loaded)?;
        Ok(())
    }
}

fn lua_lam_get<R>(_: &Lua, lam: &LuaLam<R>, key: String) -> LuaResult<LamValue>
where
    R: BufRead,
{
    let Some(store) = &lam.store else {
        return Ok(LamValue::None);
    };
    if let Ok(v) = store.get(key.as_str()) {
        return Ok(v.clone());
    }
    Ok(LamValue::None)
}

fn lua_lam_set<R>(_: &Lua, lam: &LuaLam<R>, (key, value): (String, LamValue)) -> LuaResult<LamValue>
where
    R: BufRead,
{
    let Some(store) = &lam.store else {
        return Ok(LamValue::None);
    };
    store.insert(key, &value).into_lua_err()?;
    Ok(value)
}

fn lua_lam_update<'lua, R>(
    vm: &'lua Lua,
    lam: &LuaLam<R>,
    (key, f, default_v): (String, LuaFunction<'lua>, Option<LamValue>),
) -> LuaResult<LamValue>
where
    R: BufRead,
{
    let update_fn = |old: &mut LamValue| -> LuaResult<()> {
        let old_v = vm.to_value(old)?;
        let new = f.call::<_, LamValue>(old_v)?;
        *old = new;
        Ok(())
    };

    let Some(store) = &lam.store else {
        return Ok(LamValue::None);
    };

    store.update(key, update_fn, default_v).into_lua_err()
}

impl<R> LuaUserData for LuaLam<R>
where
    for<'lua> R: 'lua + BufRead,
{
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("APP_VERSION"));
        fields.add_field_method_get("request", |vm, this| {
            let Some(m) = &this.state else {
                return Ok(LuaNil);
            };
            let Some(v) = m.get(&LamStateKey::Request) else {
                return Ok(LuaNil);
            };
            vm.to_value(&*v)
        });
    }

    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get", lua_lam_get);
        methods.add_method_mut("read", |vm, this, f| lua_lam_read(vm, &mut this.input, f));
        methods.add_method_mut("read_unicode", |vm, this, f| {
            lua_lam_read_unicode(vm, &mut this.input, f)
        });
        methods.add_method("set", lua_lam_set);
        methods.add_method("update", lua_lam_update);
    }
}

#[cfg(test)]
mod tests {
    use mockito::Server;
    use serde_json::{json, Value};
    use std::io::empty;
    use test_case::test_case;

    use crate::{EvaluationBuilder, LamValue};

    #[test]
    fn http_get() {
        let mut server = Server::new();

        let body = "<html>content</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header("content-type", "text/html")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lam/http')
            local res = m:fetch('{url}/html')
            return res:read('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(body), res.result);

        get_mock.assert();
    }

    #[test]
    fn http_get_json() {
        let mut server = Server::new();

        let body = r#"{"a":1}"#;
        let get_mock = server
            .mock("GET", "/json")
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lam/http')
            local j = require('@lam/json')
            local res = m:fetch('{url}/json')
            return j:encode(res:json())
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();

        let actual: Value = serde_json::from_str(&res.result.to_string()).unwrap();
        let expected = json!({ "a": 1 });
        assert_eq!(expected, actual);

        get_mock.assert();
    }

    #[test]
    fn http_post() {
        let mut server = Server::new();

        let post_mock = server
            .mock("POST", "/add")
            .match_body("1+1")
            .with_header("content-type", "text/plain")
            .with_body("2")
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lam/http')
            local res = m:fetch('{url}/add', {{
              method = 'POST',
              body = '1+1',
            }})
            return res:read('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("2"), res.result);

        post_mock.assert();
    }

    #[test]
    fn read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let script = r#"
        local s = require('@lam'):read('*a')
        local t = {}
        for b in (s or ""):gmatch('.') do
          table.insert(t, string.byte(b))
        end
        return t
        "#;
        let e = EvaluationBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(
            LamValue::from(vec![1.into(), 2.into(), 3.into()]),
            res.result
        );
    }

    #[test_case("assert(not require('@lam'):read('*a'))")]
    #[test_case("assert(not require('@lam'):read('*l'))")]
    #[test_case("assert(not require('@lam'):read('*n'))")]
    #[test_case("assert(not require('@lam'):read(1))")]
    fn read_empty(script: &'static str) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let _ = e.evaluate().expect(script);
    }

    #[test_case("1", 1.into())]
    #[test_case("1.2", 1.2.into())]
    #[test_case("1.23e-10", 0.000000000123.into())]
    #[test_case("", LamValue::None)]
    #[test_case("x", LamValue::None)]
    #[test_case("1\n", 1.into())]
    fn read_number(input: &'static str, expected: LamValue) {
        let script = "return require('@lam'):read('*n')";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().expect(input);
        assert_eq!(expected, res.result);
    }

    #[test_case("return require('@lam'):read('*a')", "foo\nbar".into())]
    #[test_case("return require('@lam'):read('*l')", "foo".into())]
    #[test_case("return require('@lam'):read(1)", "f".into())]
    #[test_case("return require('@lam'):read(4)", "foo\n".into())]
    fn read_string(script: &str, expected: LamValue) {
        let input = "foo\nbar";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result);
    }

    #[test_case(1, "你")]
    #[test_case(2, "你好")]
    #[test_case(3, "你好")]
    fn read_unicode_cjk_characters(n: usize, expected: &str) {
        let script = format!("return require('@lam'):read_unicode({n})");
        let input = "你好";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }

    #[test]
    fn read_unicode_cjk_characters_sequentially() {
        let input = "你好";
        let script = "return require('@lam'):read_unicode(1)";

        let e = EvaluationBuilder::new(script, input.as_bytes()).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("你"), res.result);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from("好"), res.result);

        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);
    }

    #[test]
    fn read_unicode_invalid_sequence() {
        // ref: https://www.php.net/manual/en/reference.pcre.pattern.modifiers.php#54805
        let input: &[u8] = &[0xf0, 0x28, 0x8c, 0xbc];
        let script = "return require('@lam'):read_unicode(1)";
        let e = EvaluationBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);
    }

    #[test]
    fn read_unicode_mixed_characters() {
        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let script = "return require('@lam'):read_unicode(12)";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(input), res.result);
    }

    #[test_case(1, "a")]
    #[test_case(2, "ab")]
    #[test_case(3, "ab")]
    fn read_unicode_non_cjk_characters(n: usize, expected: &str) {
        let input = "ab";
        let script = format!("return require('@lam'):read_unicode({n})");
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }

    #[test]
    fn sha256() {
        let input = "lam";
        let script = r#"
        local m = require('@lam');
        return require('@lam/crypto'):sha256(m:read('*a'))
        "#;
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        let expected = "7f1b55b860590406f84f9394f4e73356902dad022a8cd6f43221086d3c70699e";
        assert_eq!(LamValue::from(expected), res.result);
    }

    #[test]
    fn hmac_sha256() {
        let input = "lam";
        let script = r#"
        local m = require('@lam');
        return require('@lam/crypto'):hmac("sha256",m:read('*a'),"secret")
        "#;
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        let expected = "8ef120dc5b07ab464dae787f89077001dbf720132277132e7db9af154f2221a4";
        assert_eq!(LamValue::from(expected), res.result);
    }
}
