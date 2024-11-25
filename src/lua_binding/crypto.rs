use hmac::{Hmac, Mac};
use mlua::prelude::*;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

type HmacSha256 = Hmac<Sha256>;
type HmacSha512 = Hmac<Sha512>;

/// Cryptography module
pub struct LuaModCrypto {}

impl LuaUserData for LuaModCrypto {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("sha1", |_, _, payload: String| {
            Ok(base16ct::lower::encode_string(&Sha1::digest(
                payload.as_bytes(),
            )))
        });
        methods.add_method("sha256", |_, _, payload: String| {
            Ok(base16ct::lower::encode_string(&Sha256::digest(
                payload.as_bytes(),
            )))
        });
        methods.add_method("sha384", |_, _, payload: String| {
            Ok(base16ct::lower::encode_string(&Sha384::digest(
                payload.as_bytes(),
            )))
        });
        methods.add_method("sha512", |_, _, payload: String| {
            Ok(base16ct::lower::encode_string(&Sha512::digest(
                payload.as_bytes(),
            )))
        });
        methods.add_method(
            "hmac",
            |_, _, (alg, payload, secret): (String, String, String)| match alg.as_str() {
                "sha256" => {
                    let mut hasher =
                        HmacSha256::new_from_slice(secret.as_bytes()).into_lua_err()?;
                    hasher.update(payload.as_bytes());
                    let hash = hasher.finalize().into_bytes();
                    Ok(base16ct::lower::encode_string(&hash))
                }
                "sha512" => {
                    let mut hasher =
                        HmacSha512::new_from_slice(secret.as_bytes()).into_lua_err()?;
                    hasher.update(payload.as_bytes());
                    let hash = hasher.finalize().into_bytes();
                    Ok(base16ct::lower::encode_string(&hash))
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
        let e = EvaluationBuilder::new(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().unwrap();
        let expected = "8d8985d04b7abd32cbaa3779a3daa019e0d269a22aec15af8e7296f702cc68c6";
        assert_eq!(&json!(expected), res.payload());
    }

    #[test]
    fn sha256() {
        let input = "input";
        let script = "return require('@lmb/crypto'):sha256(io.read('*a'))";
        let e = EvaluationBuilder::new(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().unwrap();
        let expected = "c96c6d5be8d08a12e7b5cdc1b207fa6b2430974c86803d8891675e76fd992c20";
        assert_eq!(&json!(expected), res.payload());
    }
}
