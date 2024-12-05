use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut};
use base64::prelude::*;
use crypto_common::{KeyInit, KeyIvInit as _};
use hmac::{Hmac, Mac};
use md5::Md5;
use mlua::prelude::*;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

fn hash<H: Digest>(payload: String) -> String {
    base16ct::lower::encode_string(&H::digest(payload.as_bytes()))
}

fn compute_hmac<T: Mac + KeyInit>(secret: &str, payload: &str) -> mlua::Result<String> {
    let mut hasher = <T as KeyInit>::new_from_slice(secret.as_bytes()).into_lua_err()?;
    hasher.update(payload.as_bytes());
    let hash = hasher.finalize().into_bytes();
    Ok(base16ct::lower::encode_string(&hash))
}

/// Cryptography module
pub struct LuaModCrypto {}

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type DesCbcEnc = cbc::Encryptor<des::Des>;
type DesCbcDec = cbc::Decryptor<des::Des>;
type DesEcbEnc = ecb::Encryptor<des::Des>;
type DesEcbDec = ecb::Decryptor<des::Des>;

impl LuaUserData for LuaModCrypto {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("base64_encode", |_, _, data: String| {
            Ok(BASE64_STANDARD.encode(data.as_bytes()))
        });
        methods.add_method("base64_decode", |_, _, data: String| {
            let decoded = BASE64_STANDARD.decode(data.as_bytes()).into_lua_err()?;
            Ok(String::from_utf8(decoded).into_lua_err()?)
        });
        methods.add_method("crc32", |_, _, data: String| {
            Ok(format!("{:x}", crc32fast::hash(data.as_bytes())))
        });
        methods.add_method("md5", |_, _, data: String| Ok(hash::<Md5>(data)));
        methods.add_method("sha1", |_, _, data: String| Ok(hash::<Sha1>(data)));
        methods.add_method("sha256", |_, _, data: String| Ok(hash::<Sha256>(data)));
        methods.add_method("sha384", |_, _, data: String| Ok(hash::<Sha384>(data)));
        methods.add_method("sha512", |_, _, data: String| Ok(hash::<Sha512>(data)));
        methods.add_method(
            "hmac",
            |_, _, (alg, data, secret): (String, String, String)| match alg.as_str() {
                "sha1" => compute_hmac::<Hmac<Sha1>>(&secret, &data),
                "sha256" => compute_hmac::<Hmac<Sha256>>(&secret, &data),
                "sha384" => compute_hmac::<Hmac<Sha384>>(&secret, &data),
                "sha512" => compute_hmac::<Hmac<Sha512>>(&secret, &data),
                _ => Err(mlua::Error::runtime(format!("unsupported algorithm {alg}"))),
            },
        );
        methods.add_method(
            "encrypt",
            |_, _, (data, method, key, iv): (String, String, String, Option<String>)| match method
                .as_str()
            {
                "aes-cbc" => {
                    let iv = iv.ok_or_else(|| mlua::Error::runtime("expect IV as 4th argument"))?;
                    let encrypted = Aes128CbcEnc::new(key.as_bytes().into(), iv.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                "des-cbc" => {
                    let iv = iv.ok_or_else(|| mlua::Error::runtime("expect IV as 4th argument"))?;
                    let encrypted = DesCbcEnc::new(key.as_bytes().into(), iv.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                "des-ecb" => {
                    let encrypted = DesEcbEnc::new(key.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                _ => Err(mlua::Error::runtime(format!("unsupported method {method}"))),
            },
        );
        methods.add_method(
            "decrypt",
            |_, _, (encrypted, method, key, iv): (String, String, String, Option<String>)| {
                match method.as_str() {
                    "aes-cbc" => {
                        let iv =
                            iv.ok_or_else(|| mlua::Error::runtime("expect IV as 4th argument"))?;
                        let data = hex::decode(&encrypted).into_lua_err()?;
                        let decrypted =
                            Aes128CbcDec::new(key.as_bytes().into(), iv.as_bytes().into())
                                .decrypt_padded_vec_mut::<Pkcs7>(&data)
                                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        Ok(String::from_utf8(decrypted).into_lua_err()?)
                    }
                    "des-cbc" => {
                        let iv =
                            iv.ok_or_else(|| mlua::Error::runtime("expect IV as 4th argument"))?;
                        let data = hex::decode(&encrypted).into_lua_err()?;
                        let decrypted = DesCbcDec::new(key.as_bytes().into(), iv.as_bytes().into())
                            .decrypt_padded_vec_mut::<Pkcs7>(&data)
                            .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        Ok(String::from_utf8(decrypted).into_lua_err()?)
                    }
                    "des-ecb" => {
                        let data = hex::decode(&encrypted).into_lua_err()?;
                        let decrypted = DesEcbDec::new(key.as_bytes().into())
                            .decrypt_padded_vec_mut::<Pkcs7>(&data)
                            .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        Ok(String::from_utf8(decrypted).into_lua_err()?)
                    }
                    _ => Err(mlua::Error::runtime(format!("unsupported method {method}"))),
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Evaluation;

    #[test]
    fn hmac_sha256() {
        let input = "input";
        let script = "return require('@lmb/crypto'):hmac('sha256', io.read('*a'), 'secret')";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        let expected = "8d8985d04b7abd32cbaa3779a3daa019e0d269a22aec15af8e7296f702cc68c6";
        assert_eq!(json!(expected), res.payload);
    }

    #[test]
    fn sha256() {
        let input = "input";
        let script = "return require('@lmb/crypto'):sha256(io.read('*a'))";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        let expected = "c96c6d5be8d08a12e7b5cdc1b207fa6b2430974c86803d8891675e76fd992c20";
        assert_eq!(json!(expected), res.payload);
    }

    #[test]
    fn encrypt_decrypt() {
        let input = " ";
        let key_iv = "0123456701234567";

        let script = format!(
            "return require('@lmb/crypto'):encrypt(io.read('*a'),'aes-cbc','{key_iv}','{key_iv}')"
        );
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();

        let expected = "b019fc0029f1ae88e96597dc0667e7c8";
        assert_eq!(json!(expected), res.payload);

        let script = format!(
            "return require('@lmb/crypto'):decrypt(io.read('*a'),'aes-cbc','{key_iv}','{key_iv}')"
        );
        let e = Evaluation::builder(script, expected.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();

        assert_eq!(json!(input), res.payload);
    }
}
