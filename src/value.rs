use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LamValue {
    None,
    Boolean(bool),
    Number(f64), // represent float and integer
    String(String),
    List(Vec<LamValue>),
    Table(HashMap<String, LamValue>),
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
impl_numeric_to_lam_value!(i8, u8, i16, u16, i32, u32, f32, i64, u64, f64);

impl From<HashMap<String, LamValue>> for LamValue {
    fn from(value: HashMap<String, LamValue>) -> Self {
        Self::Table(value)
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

impl LuaUserData for LamValue {}
