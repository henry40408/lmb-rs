use std::{
    collections::HashMap,
    io::{BufReader, Cursor, Read},
    sync::Arc,
};

use http::Method;
use mlua::prelude::*;
use parking_lot::Mutex;
use tracing::warn;
use url::Url;

use super::{lua_lam_read, lua_lam_read_unicode};
use crate::{LamInput, LamValue};

/// HTTP module
pub struct LuaLamHTTP {}

/// HTTP response
pub struct LuaLamHTTPResponse {
    charset: String,
    content_type: String,
    headers: HashMap<String, Vec<String>>,
    reader: LamInput<BufReader<Box<dyn Read + Send + Sync + 'static>>>,
    status_code: u16,
}

impl LuaUserData for LuaLamHTTPResponse {
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("charset", |_, this| Ok(this.charset.clone()));
        fields.add_field_method_get("content_type", |_, this| Ok(this.content_type.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(this.headers.clone()));
        fields.add_field_method_get("status_code", |_, this| Ok(this.status_code));
    }

    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("json", |_, this, ()| {
            if "application/json" != this.content_type {
                warn!("content type is not application/json, convert with caution");
            }
            let mut reader = this.reader.lock();
            let value: LamValue = serde_json::from_reader(&mut *reader).into_lua_err()?;
            Ok(value)
        });
        methods.add_method_mut("read", |vm, this, f: LuaValue<'lua>| {
            lua_lam_read(vm, &mut this.reader, f)
        });
        methods.add_method_mut("read_unicode", |vm, this, i: Option<usize>| {
            lua_lam_read_unicode(vm, &mut this.reader, i)
        });
    }
}

fn lua_lam_fetch(
    _: &Lua,
    _: &LuaLamHTTP,
    (uri, options): (String, Option<LuaTable<'_>>),
) -> LuaResult<LuaLamHTTPResponse> {
    let url: Url = uri.parse().into_lua_err()?;
    let method: String = options
        .as_ref()
        .and_then(|t| t.get("method").ok().map(|s: String| s))
        .unwrap_or_else(|| "GET".to_string());
    let method: Method = method.parse().unwrap_or(Method::GET);
    let res = if method.is_idempotent() {
        ureq::request(method.as_str(), url.as_str())
            .call()
            .into_lua_err()?
    } else {
        let body: String = options
            .as_ref()
            .map(|t| t.get("body").unwrap_or_default())
            .unwrap_or_default();
        ureq::request(method.as_str(), url.as_str())
            .send(Cursor::new(body))
            .into_lua_err()?
    };
    let charset = res.charset().to_string();
    let content_type = res.content_type().to_string();
    let headers = {
        let mut headers = HashMap::new();
        for name in res.headers_names() {
            let values = res
                .all(&name)
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>();
            headers.insert(name, values);
        }
        headers
    };
    let status_code = res.status();
    let reader = Arc::new(Mutex::new(BufReader::new(res.into_reader())));
    Ok(LuaLamHTTPResponse {
        charset,
        content_type,
        headers,
        reader,
        status_code,
    })
}

impl LuaUserData for LuaLamHTTP {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("fetch", lua_lam_fetch);
    }
}
