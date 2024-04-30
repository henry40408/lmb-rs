use crate::*;
use mlua::prelude::*;
use std::io::BufRead;
use tracing::field;
use tracing::{error, trace_span};

use json::*;

mod json;

// ref: https://www.lua.org/pil/8.1.html
const K_LOADED: &str = "_LOADED";

/// Interface of between Lua and Rust.
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
    for<'lua> R: BufRead + 'lua,
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
        loaded.set("@lam/json", LuaLamJSON {})?;
        vm.set_named_registry_value(K_LOADED, loaded)?;
        Ok(())
    }
}

// This function intentionally uses Lua values instead of Lam values to pass bytes as partial,
// invalid strings, allowing Lua to handle the bytes.
// For a demonstration, see 'count-bytes.lua'.
fn lua_lam_read<'lua, R>(
    vm: &'lua Lua,
    lam: &mut LuaLam<R>,
    f: LamValue,
) -> LuaResult<LuaValue<'lua>>
where
    R: BufRead + 'lua,
{
    if let LamValue::String(f) = &f {
        if f == "*a" || f == "*all" {
            // accepts *a or *all
            let mut buf = Vec::new();
            let count = lam.input.lock().read_to_end(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaValue::Nil);
            }
            return String::from_utf8(buf).into_lua_err()?.into_lua(vm);
        }
        if f == "*l" || f == "*line" {
            // accepts *l or *line
            let mut buf = String::new();
            let count = lam.input.lock().read_line(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaNil);
            }
            // in Lua, *l doesn't include newline character
            return buf.trim().into_lua(vm);
        }
        if f == "*n" || f == "*number" {
            // accepts *n or *number
            let mut buf = String::new();
            let count = lam.input.lock().read_to_string(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaNil);
            }
            return Ok(buf
                .trim()
                .parse::<f64>()
                .map(LuaValue::Number)
                .unwrap_or(LuaNil));
        }
    }

    if let LamValue::Integer(i) = &f {
        let i = *i as usize;
        let s = trace_span!("read bytes from input", count = field::Empty).entered();
        let mut buf = vec![0; i];
        let count = lam.input.lock().read(&mut buf).into_lua_err()?;
        s.record("count", count);
        if count == 0 {
            return Ok(LuaNil);
        }
        buf.truncate(count);
        // Unlike Rust strings, Lua strings may not be valid UTF-8.
        // We leverage this trait to give Lua the power to handle binary.
        return Ok(mlua::Value::String(vm.create_string(&buf)?));
    }

    let f = f.to_string();
    Err(LuaError::runtime(format!("unexpected format {f}")))
}

fn lua_lam_read_unicode<'lua, R>(
    _: &'lua Lua,
    lam: &mut LuaLam<R>,
    i: Option<usize>,
) -> LuaResult<LamValue>
where
    R: BufRead + 'lua,
{
    let mut buf = vec![];
    let mut remaining = i.unwrap_or(0);
    let mut single = 0;
    while remaining > 0 {
        let count = lam
            .input
            .lock()
            .read(std::slice::from_mut(&mut single))
            .into_lua_err()?;
        if count == 0 {
            break;
        }
        buf.extend_from_slice(std::slice::from_ref(&single));
        if std::str::from_utf8(&buf).is_ok() {
            remaining -= 1;
        }
    }
    if buf.is_empty() {
        return Ok(LamValue::None);
    }
    Ok(std::str::from_utf8(&buf)
        .ok()
        .map_or(LamValue::None, LamValue::from))
}

fn lua_lam_get<'lua, R>(_: &'lua Lua, lam: &LuaLam<R>, key: String) -> LuaResult<LamValue>
where
    R: BufRead + 'lua,
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
    match store.insert(key, &value) {
        Ok(_) => Ok(value),
        Err(err) => {
            error!(?err, "failed to insert value");
            Err(LuaError::runtime("failed to insert value"))
        }
    }
}

fn lua_lam_update<'lua, R>(
    vm: &'lua Lua,
    lam: &LuaLam<R>,
    (key, f, default_v): (String, LuaFunction<'lua>, Option<LamValue>),
) -> LuaResult<LamValue>
where
    R: BufRead + 'lua,
{
    let update_fn = |old: &mut LamValue| -> LuaResult<()> {
        let old_v = vm.to_value(old)?;
        let new = f.call::<_, LamValue>(old_v).into_lua_err()?;
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
    for<'lua> R: BufRead + 'lua,
{
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("CARGO_PKG_VERSION"));
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
        methods.add_method_mut("read", lua_lam_read);
        methods.add_method_mut("read_unicode", lua_lam_read_unicode);
        methods.add_method("set", lua_lam_set);
        methods.add_method("update", lua_lam_update);
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::io::empty;
    use test_case::test_case;

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
        let e = EvalBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(
            LamValue::from(vec![1.into(), 2.into(), 3.into()]),
            res.result
        );
    }

    #[test_case(r#"assert(not require('@lam'):read('*a'))"#)]
    #[test_case(r#"assert(not require('@lam'):read('*l'))"#)]
    #[test_case(r#"assert(not require('@lam'):read('*n'))"#)]
    #[test_case(r#"assert(not require('@lam'):read(1))"#)]
    fn read_empty(script: &'static str) {
        let e = EvalBuilder::new(script, empty()).build();
        let _ = e.evaluate().expect(script);
    }

    #[test_case("1", 1.into())]
    #[test_case("1.2", 1.2.into())]
    #[test_case("1.23e-10", 0.000000000123.into())]
    #[test_case("", LamValue::None)]
    #[test_case("x", LamValue::None)]
    #[test_case("1\n", 1.into())]
    fn read_number(input: &'static str, expected: LamValue) {
        let script = r#"return require('@lam'):read('*n')"#;
        let e = EvalBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().expect(input);
        assert_eq!(expected, res.result);
    }

    #[test_case(r#"return require('@lam'):read('*a')"#, "foo\nbar".into())]
    #[test_case(r#"return require('@lam'):read('*l')"#, "foo".into())]
    #[test_case(r#"return require('@lam'):read(1)"#, "f".into())]
    #[test_case(r#"return require('@lam'):read(4)"#, "foo\n".into())]
    fn read_string(script: &str, expected: LamValue) {
        let input = "foo\nbar";
        let e = EvalBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result);
    }

    #[test_case(1, "你")]
    #[test_case(2, "你好")]
    #[test_case(3, "你好")]
    fn read_unicode_cjk_characters(n: usize, expected: &str) {
        let script = format!("return require('@lam'):read_unicode({n})");
        let input = "你好";
        let e = EvalBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }

    #[test]
    fn read_unicode_cjk_characters_sequentially() {
        let input = "你好";
        let script = "return require('@lam'):read_unicode(1)";

        let e = EvalBuilder::new(script, input.as_bytes()).build();

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
        let script = r#"return require('@lam'):read_unicode(1)"#;
        let e = EvalBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);
    }

    #[test]
    fn read_unicode_mixed_characters() {
        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let script = r#"return require('@lam'):read_unicode(12)"#;
        let e = EvalBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(input), res.result);
    }

    #[test_case(1, "a")]
    #[test_case(2, "ab")]
    #[test_case(3, "ab")]
    fn read_unicode_non_cjk_characters(n: usize, expected: &str) {
        let input = "ab";
        let script = format!("return require('@lam'):read_unicode({n})");
        let e = EvalBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }
}
