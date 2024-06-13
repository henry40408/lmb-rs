use hmac::{Hmac, Mac};
use mlua::prelude::*;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

type HmacSha256 = Hmac<Sha256>;

/// Cryptography module
pub struct LuaModCrypto {}

fn hash_to_string(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, b| {
        let _ = write!(output, "{:02x}", b);
        output
    })
}

impl LuaUserData for LuaModCrypto {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("sha256", |_, _, payload: String| {
            let mut hasher = Sha256::default();
            hasher.update(payload.as_bytes());
            let res = hasher.finalize();
            Ok(hash_to_string(res.as_slice()))
        });
        methods.add_method(
            "hmac",
            |_, _, (alg, payload, secret): (String, String, String)| match alg.as_str() {
                "sha256" => {
                    let mut hasher =
                        HmacSha256::new_from_slice(secret.as_bytes()).into_lua_err()?;
                    hasher.update(payload.as_bytes());
                    let res = hasher.finalize().into_bytes();
                    Ok(hash_to_string(res.as_slice()))
                }
                _ => Err(mlua::Error::runtime("unsupported algorithm {alg}")),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::EvaluationBuilder;

    #[test]
    fn hmac_sha256() {
        let input = "input";
        let script = "return require('@lmb/crypto'):hmac('sha256', io.read('*a'), 'secret')";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        let expected = "8d8985d04b7abd32cbaa3779a3daa019e0d269a22aec15af8e7296f702cc68c6";
        assert_eq!(&json!(expected), res.payload());
    }

    #[test]
    fn sha256() {
        let input = "input";
        let script = "return require('@lmb/crypto'):sha256(io.read('*a'))";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        let expected = "c96c6d5be8d08a12e7b5cdc1b207fa6b2430974c86803d8891675e76fd992c20";
        assert_eq!(&json!(expected), res.payload());
    }
}
