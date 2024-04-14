use crate::{LamInput, LamStore, LamValue};
use mlua::{LuaSerdeExt as _, UserData};
use std::io::{BufRead as _, Read};
use tracing::error;

pub struct LuaLam<R>
where
    R: Read,
{
    input: LamInput<R>,
    store: LamStore,
}

impl<R> LuaLam<R>
where
    R: Read,
{
    pub fn new(input: LamInput<R>, store: LamStore) -> Self {
        Self { input, store }
    }
}

fn lua_lam_read<'lua, R>(
    vm: &'lua mlua::Lua,
    lam: &LuaLam<R>,
    f: mlua::Value<'lua>,
) -> mlua::Result<mlua::Value<'lua>>
where
    R: Read + 'lua,
{
    let mut input = lam.input.lock();
    if let Some(f) = f.as_str() {
        if f == "*a" || f == "*all" {
            // accepts *a or *all
            let mut buf = Vec::new();
            let count = input.read_to_end(&mut buf)?;
            if count == 0 {
                return Ok(mlua::Value::Nil);
            }
            let s = String::from_utf8(buf).unwrap_or_default();
            return Ok(mlua::Value::String(vm.create_string(s)?));
        }
        if f == "*l" || f == "*line" {
            // accepts *l or *line
            let mut buf = String::new();
            let count = input.read_line(&mut buf)?;
            if count == 0 {
                return Ok(mlua::Value::Nil);
            }
            // in Lua, *l doesn't include newline character
            return Ok(mlua::Value::String(vm.create_string(buf.trim_end())?));
        }
        if f == "*n" || f == "*number" {
            // accepts *n or *number
            let mut buf = String::new();
            let count = input.read_to_string(&mut buf)?;
            if count == 0 {
                return Ok(mlua::Value::Nil);
            }
            return Ok(buf
                .parse::<f64>()
                .map(mlua::Value::Number)
                .unwrap_or(mlua::Value::Nil));
        }
    }

    if let Some(i) = f.as_usize() {
        let mut buf = vec![0; i];
        let count = input.read(&mut buf)?;
        if count == 0 {
            return Ok(mlua::Value::Nil);
        }
        buf.truncate(count);
        let s = vm.create_string(buf)?;
        return Ok(mlua::Value::String(s));
    }

    let s = format!("unexpected format {f:?}");
    Err(mlua::Error::RuntimeError(s))
}

fn lua_lam_read_unicode<'lua, R>(
    _: &mlua::Lua,
    lam: &LuaLam<R>,
    i: Option<u64>,
) -> mlua::Result<Option<String>>
where
    R: Read + 'lua,
{
    let mut input = lam.input.lock();
    let mut expected_read = i.unwrap_or(1);
    let mut buf = Vec::new();
    let mut byte_buf = vec![0; 1];
    loop {
        if expected_read == 0 {
            let s = String::from_utf8(buf).unwrap_or_default();
            return Ok(Some(s));
        }
        let read_bytes = input.read(&mut byte_buf)?;
        if read_bytes == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            let s = String::from_utf8(buf).unwrap_or_default();
            return Ok(Some(s));
        }
        if read_bytes > 0 {
            buf.extend_from_slice(&byte_buf);
        }
        if std::str::from_utf8(&buf).is_ok() {
            expected_read -= 1;
        }
    }
}

fn lua_lam_get<'lua, R>(
    vm: &'lua mlua::Lua,
    lam: &LuaLam<R>,
    key: String,
) -> mlua::Result<mlua::Value<'lua>>
where
    R: Read + 'lua,
{
    if let Ok(v) = lam.store.get(key.as_str()) {
        return vm.to_value(&v.clone());
    }
    Ok(mlua::Value::Nil)
}

fn lua_lam_set<'lua, R>(
    vm: &'lua mlua::Lua,
    lam: &LuaLam<R>,
    (key, value): (String, mlua::Value<'lua>),
) -> mlua::Result<mlua::Value<'lua>>
where
    R: Read,
{
    match lam.store.insert(key, &vm.from_value(value.clone())?) {
        Ok(_) => Ok(value),
        Err(err) => {
            error!(?err, "failed to insert value");
            Err(mlua::Error::RuntimeError(
                "failed to insert value".to_string(),
            ))
        }
    }
}

fn lua_lam_update<'lua, R>(
    vm: &'lua mlua::Lua,
    lam: &LuaLam<R>,
    (key, f, default_v): (String, mlua::Function<'lua>, mlua::Value<'lua>),
) -> mlua::Result<mlua::Value<'lua>>
where
    R: Read + 'lua,
{
    let g = |old: &mut LamValue| {
        let old_v = match vm.to_value(old) {
            Ok(v) => v,
            Err(err) => {
                error!(?err, "failed to convert store value");
                return;
            }
        };
        let new_v = match f.call(old_v) {
            Ok(v) => v,
            Err(err) => {
                error!(?err, "failed to run the function");
                return;
            }
        };
        let new = match vm.from_value(new_v) {
            Ok(v) => v,
            Err(err) => {
                error!(?err, "failed to convert new value");
                return;
            }
        };
        *old = new;
    };

    let v = lam
        .store
        .update(key, g, &vm.from_value(default_v)?)
        .map_err(|err| {
            error!(?err, "failed to update value");
            mlua::Error::RuntimeError("failed to update value".to_string())
        })?;
    vm.to_value(&v)
}

impl<R> UserData for LuaLam<R>
where
    for<'lua> R: Read + 'lua,
{
    fn add_fields<'lua, F: mlua::prelude::LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("CARGO_PKG_VERSION"));
    }

    fn add_methods<'lua, M: mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get", lua_lam_get);
        methods.add_method("read", lua_lam_read);
        methods.add_method("read_unicode", lua_lam_read_unicode);
        methods.add_method("set", lua_lam_set);
        methods.add_method("update", lua_lam_update);
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use crate::{evaluate, EvalBuilder, LamValue};

    #[test]
    fn test_read() {
        let cases = [
            [r#"return require('@lam'):read('*a')"#, "foo\nbar"],
            [r#"return require('@lam'):read('*l')"#, "foo"],
            [r#"return require('@lam'):read(1)"#, "f"],
            [r#"return require('@lam'):read(4)"#, "foo\n"],
        ];
        for case in cases {
            let input = "foo\nbar";
            let [script, expected] = case;
            let e = EvalBuilder::new(Cursor::new(input), script).build();
            let res = evaluate(&e).expect(script);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }

        let script = r#"return require('@lam'):read('*n')"#;
        let cases = [
            ["1", "1"],
            ["1.2", "1.2"],
            ["1.23e-10", "0.000000000123"],
            ["3.1415926", "3.1415926"],
            ["", ""],
            ["NaN", "NaN"],
            ["InvalidNumber", ""],
        ];
        for case in cases {
            let [input, expected] = case;
            let e = EvalBuilder::new(Cursor::new(input), script).build();
            let res = evaluate(&e).expect(input);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn test_read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let e = EvalBuilder::new(input, r#"return #require('@lam'):read('*a')"#).build();
        let res = evaluate(&e).unwrap();
        assert_eq!(LamValue::Number(3f64), res.result);
    }

    #[test]
    fn test_read_empty() {
        let scripts = [
            r#"assert(not require('@lam'):read('*a'))"#,
            r#"assert(not require('@lam'):read('*l'))"#,
            r#"assert(not require('@lam'):read('*n'))"#,
            r#"assert(not require('@lam'):read(1))"#,
        ];
        for script in scripts {
            let input: &[u8] = &[];
            let e = EvalBuilder::new(input, script).build();
            let _ = evaluate(&e).expect(script);
        }
    }

    #[test]
    fn test_read_unicode() {
        let input = "你好";
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"return require('@lam'):read_unicode(1)"#,
        )
        .build();
        let res = evaluate(&e).unwrap();
        assert_eq!(LamValue::String("你".to_string()), res.result);

        let input = r#"{"key":"你好"}"#;
        let e = EvalBuilder::new(
            Cursor::new(input),
            r#"return require('@lam'):read_unicode(12)"#,
        )
        .build();
        let res = evaluate(&e).unwrap();
        assert_eq!(input, res.result.to_string());
    }
}
