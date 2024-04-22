use crate::*;
use mlua::prelude::*;
use std::io::{BufRead as _, Read};
use tracing::error;

const K_LOADED: &str = "_LOADED";

pub struct LuaLam<R>
where
    R: Read,
{
    input: Option<LamInput<R>>,
    store: Option<LamStore>,
}

impl<R> LuaLam<R>
where
    for<'lua> R: Read + 'lua,
{
    pub fn new(input: Option<LamInput<R>>, store: Option<LamStore>) -> Self {
        Self { input, store }
    }

    pub fn register(
        vm: &Lua,
        input: Option<LamInput<R>>,
        store: Option<LamStore>,
    ) -> LamResult<()> {
        let loaded = vm.named_registry_value::<LuaTable<'_>>(K_LOADED)?;
        loaded.set("@lam", Self::new(input, store))?;
        vm.set_named_registry_value(K_LOADED, loaded)?;
        Ok(())
    }
}

fn lua_lam_read<'lua, R>(
    vm: &'lua Lua,
    lam: &mut LuaLam<R>,
    f: LuaValue<'lua>,
) -> LuaResult<LuaValue<'lua>>
where
    R: Read + 'lua,
{
    let Some(input) = &mut lam.input else {
        return Ok(LuaNil);
    };
    if let Some(f) = f.as_str() {
        if f == "*a" || f == "*all" {
            // accepts *a or *all
            let mut buf = Vec::new();
            let count = input.read_to_end(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaValue::Nil);
            }
            return String::from_utf8(buf).into_lua_err()?.into_lua(vm);
        }
        if f == "*l" || f == "*line" {
            // accepts *l or *line
            let mut buf = String::new();
            let count = input.read_line(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaNil);
            }
            // in Lua, *l doesn't include newline character
            return buf.trim().into_lua(vm);
        }
        if f == "*n" || f == "*number" {
            // accepts *n or *number
            let mut buf = String::new();
            let count = input.read_to_string(&mut buf).into_lua_err()?;
            if count == 0 {
                return Ok(LuaNil);
            }
            return Ok(buf.parse::<f64>().map(LuaValue::Number).unwrap_or(LuaNil));
        }
    }

    if let Some(i) = f.as_usize() {
        let mut buf = Vec::with_capacity(i);
        let count = input
            .by_ref()
            .take(i as u64)
            .read_to_end(&mut buf)
            .into_lua_err()?;
        if count == 0 {
            return Ok(LuaNil);
        }
        buf.truncate(count);
        return String::from_utf8(buf).into_lua_err()?.into_lua(vm);
    }

    let f = f.to_string().into_lua_err()?;
    Err(LuaError::runtime(format!("unexpected format {f}")))
}

fn lua_lam_read_unicode<'lua, R>(
    vm: &'lua Lua,
    lam: &mut LuaLam<R>,
    i: Option<usize>,
) -> LuaResult<LuaValue<'lua>>
where
    R: Read + 'lua,
{
    let Some(input) = &mut lam.input else {
        return Ok(LuaNil);
    };
    let mut buf = vec![];
    let mut remaining = i.unwrap_or(0);
    let mut single = 0;
    while remaining > 0 {
        let count = input
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
        return vm.to_value(&LamValue::None);
    }
    vm.to_value(&std::str::from_utf8(&buf).ok())
}

fn lua_lam_get<'lua, R>(vm: &'lua Lua, lam: &LuaLam<R>, key: String) -> LuaResult<LuaValue<'lua>>
where
    R: Read + 'lua,
{
    let Some(store) = &lam.store else {
        return Ok(LuaNil);
    };
    if let Ok(v) = store.get(key.as_str()) {
        return vm.to_value(&v.clone());
    }
    Ok(LuaNil)
}

fn lua_lam_set<'lua, R>(
    vm: &'lua Lua,
    lam: &LuaLam<R>,
    (key, value): (String, LuaValue<'lua>),
) -> LuaResult<LuaValue<'lua>>
where
    R: Read,
{
    let Some(store) = &lam.store else {
        return Ok(LuaNil);
    };
    match store.insert(key, &vm.from_value(value.clone())?) {
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
    (key, f, default_v): (String, LuaFunction<'lua>, LuaValue<'lua>),
) -> LuaResult<LuaValue<'lua>>
where
    R: Read + 'lua,
{
    let g = |old: &mut LamValue| -> LuaResult<()> {
        let old_v = vm.to_value(old)?;
        let new_v = f.call(old_v).into_lua_err()?;
        let new = vm.from_value(new_v)?;
        *old = new;
        Ok(())
    };

    let Some(store) = &lam.store else {
        // the function throws an error instead of returing a new value,
        // return the old value instead.
        return Ok(LuaNil);
    };

    let v = store
        .update(key, g, &vm.from_value(default_v)?)
        .into_lua_err()?;
    vm.to_value(&v)
}

impl<R> LuaUserData for LuaLam<R>
where
    for<'lua> R: Read + 'lua,
{
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("CARGO_PKG_VERSION"));
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
    use test_case::test_case;

    #[test]
    fn read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let e = EvalBuilder::new(r#"return #require('@lam'):read('*a')"#.into())
            .set_input(Some(input))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::Number(3f64), res.result);
    }

    #[test_case(r#"assert(not require('@lam'):read('*a'))"#)]
    #[test_case(r#"assert(not require('@lam'):read('*l'))"#)]
    #[test_case(r#"assert(not require('@lam'):read('*n'))"#)]
    #[test_case(r#"assert(not require('@lam'):read(1))"#)]
    fn read_empty(script: &'static str) {
        let e = EvalBuilder::new(script.into()).build();
        let _ = e.evaluate().expect(script);
    }

    #[test_case("1", 1.into())]
    #[test_case("1.2", 1.2.into())]
    #[test_case("1.23e-10", 0.000000000123.into())]
    #[test_case("", LamValue::None)]
    #[test_case("x", LamValue::None)]
    fn read_number(input: &'static str, expected: LamValue) {
        let script = r#"return require('@lam'):read('*n')"#;
        let e = EvalBuilder::new(script.into())
            .set_input(Some(input.as_bytes()))
            .build();
        let res = e.evaluate().expect(input);
        assert_eq!(expected, res.result);
    }

    #[test_case(r#"return require('@lam'):read('*a')"#, "foo\nbar".into())]
    #[test_case(r#"return require('@lam'):read('*l')"#, "foo".into())]
    #[test_case(r#"return require('@lam'):read(1)"#, "f".into())]
    #[test_case(r#"return require('@lam'):read(4)"#, "foo\n".into())]
    fn read_string(script: &str, expected: LamValue) {
        let input = "foo\nbar";
        let e = EvalBuilder::new(script.into())
            .set_input(Some(input.as_bytes()))
            .build();
        let res = e.evaluate().expect(script);
        assert_eq!(expected, res.result);
    }

    #[test_case(1, "你")]
    #[test_case(2, "你好")]
    #[test_case(3, "你好")]
    fn read_unicode_cjk_characters(n: usize, expected: &str) {
        let input = "你好";
        let e = EvalBuilder::new(format!("return require('@lam'):read_unicode({n})").into())
            .set_input(Some(input.as_bytes()))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }

    #[test]
    fn read_unicode_cjk_characters_sequentially() {
        let input = "你好";
        let script = "return require('@lam'):read_unicode(1)";

        let e = EvalBuilder::new(script.into())
            .set_input(Some(input.as_bytes()))
            .build();

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
        let e = EvalBuilder::new(r#"return require('@lam'):read_unicode(1)"#.into())
            .set_input(Some(input))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::None, res.result);
    }

    #[test]
    fn read_unicode_mixed_characters() {
        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let e = EvalBuilder::new(r#"return require('@lam'):read_unicode(12)"#.into())
            .set_input(Some(input.as_bytes()))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(input, res.result.to_string());
    }

    #[test_case(1, "a")]
    #[test_case(2, "ab")]
    #[test_case(3, "ab")]
    fn read_unicode_non_cjk_characters(n: usize, expected: &str) {
        let input = "ab";
        let script = format!("return require('@lam'):read_unicode({n})");
        let e = EvalBuilder::new(script.into())
            .set_input(Some(input.as_bytes()))
            .build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::from(expected), res.result);
    }
}
