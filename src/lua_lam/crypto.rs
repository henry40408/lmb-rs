use hmac::{Hmac, Mac};
use mlua::prelude::*;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

type HmacSha256 = Hmac<Sha256>;

/// Cryptography module
pub struct LuaLamCrypto {}

fn hash_to_string(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, b| {
        let _ = write!(output, "{:02x}", b);
        output
    })
}

impl LuaUserData for LuaLamCrypto {
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
