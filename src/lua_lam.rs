use crate::*;
use mlua::{LuaSerdeExt as _, UserData};
use std::io::{BufRead, Read};
use tracing::error;
use unicode_segmentation::UnicodeSegmentation;

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
    if let Some(f) = f.as_str() {
        if f == "*a" || f == "*all" {
            // accepts *a or *all
            let mut buf = Vec::new();
            let count = lam.input.lock().read_to_end(&mut buf)?;
            if count == 0 {
                return Ok(mlua::Value::Nil);
            }
            let s = String::from_utf8(buf).unwrap_or_default();
            return Ok(mlua::Value::String(vm.create_string(s)?));
        }
        if f == "*l" || f == "*line" {
            // accepts *l or *line
            let mut buf = String::new();
            let count = lam.input.lock().read_line(&mut buf)?;
            if count == 0 {
                return Ok(mlua::Value::Nil);
            }
            // in Lua, *l doesn't include newline character
            return Ok(mlua::Value::String(vm.create_string(buf.trim_end())?));
        }
        if f == "*n" || f == "*number" {
            // accepts *n or *number
            let mut buf = String::new();
            let count = lam.input.lock().read_to_string(&mut buf)?;
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
        let mut buf = Vec::with_capacity(i);
        let count = lam
            .input
            .lock()
            .by_ref()
            .take(i as u64)
            .read_to_end(&mut buf)?;
        if count == 0 {
            return Ok(mlua::Value::Nil);
        }
        buf.truncate(count);
        let s = vm.create_string(buf)?;
        return Ok(mlua::Value::String(s));
    }

    let f = f.as_str().unwrap_or("?");
    Err(mlua::Error::runtime(format!("unexpected format {f}")))
}

fn lua_lam_read_unicode<'lua, R>(
    vm: &'lua mlua::Lua,
    lam: &LuaLam<R>,
    i: Option<usize>,
) -> mlua::Result<mlua::Value<'lua>>
where
    R: Read + 'lua,
{
    let mut locked = lam.input.lock();
    let mut s = String::new();
    let mut remaining = i.unwrap_or(1);
    while let Ok(buffer) = locked.fill_buf() {
        let (input, to_consume) = match std::str::from_utf8(buffer) {
            Ok(input) => (input, input.len()),
            Err(e) => {
                let to_consume = e.valid_up_to();
                if to_consume == 0 {
                    break;
                }
                let input = std::str::from_utf8(&buffer[..to_consume])?;
                (input, to_consume)
            }
        };
        if to_consume == 0 {
            break;
        }

        let part = input.chars().take(remaining).collect::<String>();
        remaining -= part.graphemes(true).count();
        s.push_str(&part);
        if remaining == 0 {
            break;
        }

        locked.consume(to_consume);
    }
    vm.to_value(&s)
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
            Err(mlua::Error::runtime("failed to insert value"))
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
            mlua::Error::runtime("failed to update value")
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
mod tests {
    use crate::*;

    #[test]
    fn read() {
        let cases = [
            [r#"return require('@lam'):read('*a')"#, "foo\nbar"],
            [r#"return require('@lam'):read('*l')"#, "foo"],
            [r#"return require('@lam'):read(1)"#, "f"],
            [r#"return require('@lam'):read(4)"#, "foo\n"],
        ];
        for case in cases {
            let input = "foo\nbar";
            let [script, expected] = case;
            let e = EvalBuilder::new(input.as_bytes(), script).build();
            let res = e.evaluate().expect(script);
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
            let e = EvalBuilder::new(input.as_bytes(), script).build();
            let res = e.evaluate().expect(input);
            assert_eq!(
                expected,
                res.result.to_string(),
                "expect result of {script} to equal to {expected}"
            );
        }
    }

    #[test]
    fn read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let e = EvalBuilder::new(input, r#"return #require('@lam'):read('*a')"#).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::Number(3f64), res.result);
    }

    #[test]
    fn read_empty() {
        let scripts = [
            r#"assert(not require('@lam'):read('*a'))"#,
            r#"assert(not require('@lam'):read('*l'))"#,
            r#"assert(not require('@lam'):read('*n'))"#,
            r#"assert(not require('@lam'):read(1))"#,
        ];
        for script in scripts {
            let input: &[u8] = &[];
            let e = EvalBuilder::new(input, script).build();
            let _ = e.evaluate().expect(script);
        }
    }

    #[test]
    fn read_unicode() {
        // non-CJK characters
        let input = "ab";
        let cases = [(1, "a"), (2, "ab"), (3, "ab")];
        for (count, expected) in cases {
            let e = EvalBuilder::new(
                input.as_bytes(),
                format!("return require('@lam'):read_unicode({count})"),
            )
            .build();
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::String(expected.to_string()), res.result);
        }

        // CJK characters
        let input = "你好";
        let cases = [(1, "你"), (2, "你好"), (3, "你好")];
        for (count, expected) in cases {
            let e = EvalBuilder::new(
                input.as_bytes(),
                format!("return require('@lam'):read_unicode({count})"),
            )
            .build();
            let res = e.evaluate().unwrap();
            assert_eq!(LamValue::String(expected.to_string()), res.result);
        }

        // invalid unicode sequence
        // ref: https://www.php.net/manual/en/reference.pcre.pattern.modifiers.php#54805
        let input: &[u8] = &[0xf0, 0x28, 0x8c, 0xbc];
        let e = EvalBuilder::new(input, r#"return require('@lam'):read_unicode(1)"#).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LamValue::String(String::new()), res.result);

        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let e = EvalBuilder::new(
            input.as_bytes(),
            r#"return require('@lam'):read_unicode(12)"#,
        )
        .build();
        let res = e.evaluate().unwrap();
        assert_eq!(input, res.result.to_string());
    }
}
