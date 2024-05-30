use std::{
    collections::HashMap,
    io::{BufReader, Cursor, Read},
    sync::Arc,
};

use http::{Method, StatusCode};
use mlua::prelude::*;
use parking_lot::Mutex;
use tracing::{trace, trace_span, warn};
use ureq::Request;
use url::Url;

use super::{lua_lmb_read, lua_lmb_read_unicode};
use crate::{LmbInput, LmbValue};

/// HTTP module
pub struct LuaLmbHTTP {}

/// HTTP response
pub struct LuaLmbHTTPResponse {
    charset: String,
    content_type: String,
    headers: HashMap<String, Vec<String>>,
    reader: LmbInput<BufReader<Box<dyn Read + Send + Sync + 'static>>>,
    status_code: StatusCode,
}

impl LuaUserData for LuaLmbHTTPResponse {
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("charset", |_, this| Ok(this.charset.clone()));
        fields.add_field_method_get("content_type", |_, this| Ok(this.content_type.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(this.headers.clone()));
        fields.add_field_method_get("ok", |_, this| Ok(this.status_code.is_success()));
        fields.add_field_method_get("status_code", |_, this| Ok(this.status_code.as_u16()));
    }

    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("json", |_, this, ()| {
            if "application/json" != this.content_type {
                warn!("content type is not application/json, convert with caution");
            }
            let mut reader = this.reader.lock();
            let value: LmbValue = serde_json::from_reader(&mut *reader).into_lua_err()?;
            Ok(value)
        });
        methods.add_method("read", |vm, this, f: Option<LuaValue<'lua>>| {
            lua_lmb_read(vm, &this.reader, f)
        });
        methods.add_method("read_unicode", |vm, this, f: LuaValue<'lua>| {
            lua_lmb_read_unicode(vm, &this.reader, f)
        });
    }
}

fn set_headers(req: Request, headers: &LmbValue) -> Request {
    let LmbValue::Table(h) = headers else {
        return req;
    };
    let mut new_req = req;
    for (k, v) in h.iter() {
        new_req = new_req.set(k.as_str(), &v.to_string());
    }
    new_req
}

fn lua_lmb_fetch(
    _: &Lua,
    _: &LuaLmbHTTP,
    (uri, options): (String, Option<LuaTable<'_>>),
) -> LuaResult<LuaLmbHTTPResponse> {
    let options = options.as_ref();
    let url: Url = uri.parse().into_lua_err()?;
    let method: String = options
        .and_then(|t| t.get("method").ok().map(|s: String| s))
        .unwrap_or_else(|| "GET".to_string());
    let method: Method = method.parse().unwrap_or(Method::GET);
    let headers: LmbValue = options
        .and_then(|t| t.get("headers").ok())
        .unwrap_or(LmbValue::None);
    let _s = trace_span!("send_http_request", %method, %url, ?headers).entered();
    let res = if method.is_safe() {
        let req = ureq::request_url(method.as_str(), &url);
        let req = set_headers(req, &headers);
        req.call()
    } else {
        let body: String = options
            .map(|t| t.get("body").unwrap_or_default())
            .unwrap_or_default();
        let req = ureq::request_url(method.as_str(), &url);
        let req = set_headers(req, &headers);
        req.send(Cursor::new(body))
    };
    let res = match res {
        Ok(res) | Err(ureq::Error::Status(_, res)) => res,
        Err(e) => return Err(e.into_lua_err()),
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
    let status_code = StatusCode::from_u16(res.status()).into_lua_err()?;
    trace!(%status_code, charset, content_type, "response");
    let reader = Arc::new(Mutex::new(BufReader::new(res.into_reader())));
    Ok(LuaLmbHTTPResponse {
        charset,
        content_type,
        headers,
        reader,
        status_code,
    })
}

impl LuaUserData for LuaLmbHTTP {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("fetch", lua_lmb_fetch);
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use mockito::Server;
    use serde_json::{json, Value};

    use crate::{EvaluationBuilder, LmbValue};

    #[test]
    fn http_get() {
        let mut server = Server::new();

        let body = "<html>content</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header("content-type", "text/html")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/html')
            return res:read('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_headers() {
        let mut server = Server::new();

        let body = "a";
        let get_mock = server
            .mock("GET", "/headers")
            .match_header("a", "b")
            .with_header("content-type", "text/plain")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/headers', {{ headers = {{ a = 'b' }} }})
            return res:read('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_unicode() {
        let mut server = Server::new();

        let body = "<html>中文</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header("content-type", "text/html")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/html')
            return res:read_unicode('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_json() {
        let mut server = Server::new();

        let body = r#"{"a":1}"#;
        let get_mock = server
            .mock("GET", "/json")
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local j = require('@lmb/json')
            local res = m:fetch('{url}/json')
            return j:encode(res:json())
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();

        let actual: Value = serde_json::from_str(&res.payload.to_string()).unwrap();
        let expected = json!({ "a": 1 });
        assert_eq!(expected, actual);

        get_mock.assert();
    }

    #[test]
    fn http_post() {
        let mut server = Server::new();

        let post_mock = server
            .mock("POST", "/add")
            .match_body("1+1")
            .with_header("content-type", "text/plain")
            .with_body("2")
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/add', {{
              method = 'POST',
              body = '1+1',
            }})
            return res:read('*a')
            "#
        );
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from("2"), res.payload);

        post_mock.assert();
    }
}
