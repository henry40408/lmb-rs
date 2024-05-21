use std::io::BufRead;

use mlua::prelude::*;
use tracing::{field, trace_span};

use crate::LamInput;

// This function intentionally uses Lua values instead of Lam values to pass bytes as partial,
// invalid strings, allowing Lua to handle the bytes.
// For a demonstration, see "count-bytes.lua".
pub(crate) fn lua_lam_read<'lua, R>(
    vm: &'lua Lua,
    input: &mut LamInput<R>,
    f: LuaValue<'lua>,
) -> LuaResult<LuaValue<'lua>>
where
    R: BufRead,
{
    if let Some(f) = f.as_str() {
        match f {
            "*a" | "*all" => {
                // accepts *a or *all
                let mut buf = Vec::new();
                let count = input.lock().read_to_end(&mut buf)?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                return String::from_utf8(buf).into_lua_err()?.into_lua(vm);
            }
            "*l" | "*line" => {
                // accepts *l or *line
                let mut buf = String::new();
                let count = input.lock().read_line(&mut buf)?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                // in Lua, *l doesn't include newline character
                return buf.trim().into_lua(vm);
            }
            "*n" | "*number" => {
                // accepts *n or *number
                let mut buf = String::new();
                let count = input.lock().read_to_string(&mut buf)?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                return Ok(buf
                    .trim()
                    .parse::<f64>()
                    .map(LuaValue::Number)
                    .unwrap_or(LuaNil));
            }
            _ => {}
        }
    }

    if let Some(i) = f.as_usize() {
        let s = trace_span!("read bytes from input", count = field::Empty).entered();
        let mut buf = vec![0; i];
        let count = input.lock().read(&mut buf)?;
        s.record("count", count);
        if count == 0 {
            return Ok(LuaNil);
        }
        buf.truncate(count);
        // Unlike Rust strings, Lua strings may not be valid UTF-8.
        // We leverage this trait to give Lua the power to handle binary.
        return Ok(mlua::Value::String(vm.create_string(&buf)?));
    }

    let f = f.to_string()?;
    Err(LuaError::runtime(format!("unexpected format {f}")))
}

pub(crate) fn lua_lam_read_unicode<'lua, R>(
    vm: &'lua Lua,
    input: &mut LamInput<R>,
    f: LuaValue<'lua>,
) -> LuaResult<LuaValue<'lua>>
where
    R: BufRead,
{
    if let Some(f) = f.as_str() {
        match f {
            "*a" | "*all" => {
                let mut s = vec![];
                input.lock().read_to_end(&mut s).into_lua_err()?;
                return Ok(LuaValue::String(vm.create_string(s)?));
            }
            "*l" | "*line" => {
                let mut s = String::new();
                input.lock().read_line(&mut s).into_lua_err()?;
                return Ok(LuaValue::String(vm.create_string(s.trim())?));
            }
            _ => {}
        }
    }

    if let Some(n) = f.as_usize() {
        let mut remaining = n;
        let mut buf = vec![];
        let mut single = 0;
        while remaining > 0 {
            let count = input.lock().read(std::slice::from_mut(&mut single))?;
            if count == 0 {
                break;
            }
            buf.extend_from_slice(std::slice::from_ref(&single));
            if std::str::from_utf8(&buf).is_ok() {
                remaining -= 1;
            }
        }
        if buf.is_empty() {
            return Ok(LuaNil);
        }
        return Ok(std::str::from_utf8(&buf).ok().map_or_else(
            || LuaNil,
            |s| {
                vm.create_string(s)
                    .map_or_else(|_| LuaNil, LuaValue::String)
            },
        ));
    }

    let f = f.to_string()?;
    Err(mlua::Error::runtime(format!("unexpected format {f}")))
}
