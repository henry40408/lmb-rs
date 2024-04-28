use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Value mapping between Rust and Lua.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LamValue {
    /// nil in Lua, None in Rust.
    None,
    /// Boolean.
    Boolean(bool),
    /// Numbers, includes float and integer.
    Number(f64),
    /// String.
    String(String),
    /// Table without explicit key in Lua, Vec in Rust.
    List(Vec<LamValue>),
    /// Table with explicit key in Lua, HashMap in Rust.
    Table(HashMap<String, LamValue>),
}

impl<'lua> IntoLua<'lua> for LamValue {
    fn into_lua(self, lua: &'lua Lua) -> LuaResult<LuaValue<'lua>> {
        lua.to_value(&self)
    }
}

impl<'lua> FromLua<'lua> for LamValue {
    fn from_lua(value: LuaValue<'lua>, lua: &'lua Lua) -> LuaResult<Self> {
        lua.from_value(value)
    }
}

impl From<bool> for LamValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<&str> for LamValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}

macro_rules! impl_numeric_to_lam_value {
    ($($t:ty),*) => {
        $(
            impl From<$t> for LamValue {
                fn from(value: $t) -> Self { Self::Number(value as f64) }
            }
        )*
    };
}
impl_numeric_to_lam_value!(f32, f64, i8, i16, i32, i64, u8, u16, u32, u64);

impl<S> From<HashMap<S, LamValue>> for LamValue
where
    S: ToString,
{
    fn from(value: HashMap<S, LamValue>) -> Self {
        let mut h = HashMap::new();
        for (k, v) in value {
            h.insert(k.to_string(), v.clone());
        }
        Self::Table(h)
    }
}

impl From<Vec<LamValue>> for LamValue {
    fn from(value: Vec<LamValue>) -> Self {
        Self::List(value)
    }
}

impl std::fmt::Display for LamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LamValue::None => write!(f, ""),
            LamValue::Boolean(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            LamValue::Number(n) => write!(f, "{}", n),
            LamValue::String(s) => write!(f, r#"{}"#, s),
            LamValue::List(_) | LamValue::Table(_) => write!(f, "table: 0x0"),
        }
    }
}
