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
    /// Numbers that can be converted to integer.
    Integer(i64),
    /// Numbers that cannot be converted to integer but float.
    Number(f64),
    /// String.
    String(String),
    /// Table without explicit key in Lua, Vec in Rust.
    List(Vec<LamValue>),
    /// Table with explicit key in Lua, `HashMap` in Rust.
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

macro_rules! impl_float_to_lam_value {
    ($($t:ty),*) => {
        $(
            impl From<$t> for LamValue {
                fn from(value: $t) -> Self { Self::Number(value as f64) }
            }
        )*
    };
}
impl_float_to_lam_value!(f32, f64);

macro_rules! impl_integer_to_lam_value {
    ($($t:ty),*) => {
        $(
            impl From<$t> for LamValue {
                fn from(value: $t) -> Self { Self::Integer(value as i64) }
            }
        )*
    };
}
impl_integer_to_lam_value!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl<S> From<HashMap<S, LamValue>> for LamValue
where
    S: AsRef<str>,
{
    fn from(value: HashMap<S, LamValue>) -> Self {
        let mut h = HashMap::new();
        for (k, v) in value {
            h.insert(k.as_ref().to_string(), v.clone());
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
            LamValue::Integer(n) => write!(f, "{}", n),
            LamValue::Number(n) => write!(f, "{}", n),
            LamValue::String(s) => write!(f, r#"{}"#, s),
            LamValue::List(l) => write!(f, "table: {l:p}"),
            LamValue::Table(t) => write!(f, "table: {t:p}"),
        }
    }
}
