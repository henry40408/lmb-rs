use mlua::prelude::*;

use crate::LmbValue;

/// JSON module
pub struct LuaLmbJSON {}

impl LuaUserData for LuaLmbJSON {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("decode", |vm, _, value: String| {
            vm.to_value(&serde_json::from_str::<LmbValue>(&value).into_lua_err()?)
        });
        methods.add_method("encode", |_, _, value: LmbValue| {
            serde_json::to_string(&value).into_lua_err()
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{EvaluationBuilder, LmbValue};
    use serde_json::{json, Value};
    use std::io::empty;

    #[test]
    fn json_decode() {
        let script = r#"
        local m = require('@lmb/json');
        return m:decode('{"bool":true,"num":2,"str":"hello"}')
        "#;
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let expected: LmbValue = maplit::hashmap! {
            "bool" => true.into(),
            "num" => 2.into(),
            "str" => "hello".into()
        }
        .into();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn json_encode() {
        let script = r#"
        local m = require('@lmb/json');
        return m:encode({ bool = true, num = 2, str = 'hello' })
        "#;
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let value: Value = serde_json::from_str(&res.payload.to_string()).unwrap();
        assert_eq!(json!({"bool":true,"num":2,"str":"hello"}), value);
    }

    #[test]
    fn json_decode_encode() {
        // https://github.com/rxi/json.lua/issues/19
        let script = r#"
        local m = require('@lmb/json');
        return m:encode(m:decode('{"a":[{}]}'))
        "#;
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        let value: Value = serde_json::from_str(&res.payload.to_string()).unwrap();
        assert_eq!(json!({"a":[{}]}), value);
    }
}
