use crate::*;
use mlua::prelude::*;

/// JSON module for Lua
pub struct LuaLamJSON {}

impl LuaUserData for LuaLamJSON {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("decode", |vm, _, value: String| {
            vm.to_value(&serde_json::from_str::<LamValue>(&value).into_lua_err()?)
        });
        methods.add_method("encode", |_, _, value: LamValue| {
            serde_json::to_string(&value).into_lua_err()
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{EvalBuilder, LamValue};
    use serde_json::{json, Value};
    use std::io::empty;

    #[test]
    fn json_decode() {
        let script = r#"
        local m = require('@lam/json');
        return m:decode('{"bool":true,"num":2,"str":"hello"}')
        "#;
        let e = EvalBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let expected: LamValue = maplit::hashmap! {
            "bool" => true.into(),
            "num" => 2.into(),
            "str" => "hello".into()
        }
        .into();
        assert_eq!(expected, res.result);
    }

    #[test]
    fn json_encode() {
        let script = r#"
        local m = require('@lam/json');
        return m:encode({ bool = true, num = 2, str = 'hello' })
        "#;
        let e = EvalBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let value: Value = serde_json::from_str(&res.result.to_string()).unwrap();
        assert_eq!(json!({"bool":true,"num":2,"str":"hello"}), value);
    }

    #[test]
    fn json_decode_encode() {
        // https://github.com/rxi/json.lua/issues/19
        let script = r#"
        local m = require('@lam/json');
        return m:encode(m:decode('{"a":[{}]}'))
        "#;
        let e = EvalBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let value: Value = serde_json::from_str(&res.result.to_string()).unwrap();
        assert_eq!(json!({"a":[{}]}), value);
    }
}
