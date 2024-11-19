#![allow(clippy::unwrap_used)]

use bencher::{benchmark_group, benchmark_main, Bencher};
use lmb::EvaluationBuilder;
use mlua::prelude::*;
use std::io::{empty, BufReader, Cursor, Read as _};

static SCRIPT: &str = "return true";

/// evaluation

fn lmb_evaluate(bencher: &mut Bencher) {
    let e = EvaluationBuilder::new(SCRIPT, empty()).build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn mlua_call(bencher: &mut Bencher) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    let f = vm.load(SCRIPT).into_function().unwrap();
    bencher.iter(|| f.call::<_, bool>(()).unwrap());
}

fn mlua_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn mlua_sandbox_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

/// store

fn lmb_no_store(bencher: &mut Bencher) {
    let e = EvaluationBuilder::new(SCRIPT, empty()).build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn lmb_default_store(bencher: &mut Bencher) {
    let e = EvaluationBuilder::new(SCRIPT, empty())
        .default_store()
        .build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn lmb_update(bencher: &mut Bencher) {
    let script = r#"
    return require("@lmb"):update({ "a" }, function(values)
    	local a = table.unpack(values)
    	return table.pack(a + 1)
    end, { 0 })
    "#;
    let e = EvaluationBuilder::new(script, empty())
        .default_store()
        .build();
    bencher.iter(|| e.evaluate().unwrap());
}

/// read

fn lmb_read_all(bencher: &mut Bencher) {
    let input = "1";
    let script = "return io.read('*a')";
    let e = EvaluationBuilder::new(script, input.as_bytes()).build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn lmb_read_line(bencher: &mut Bencher) {
    let input = "1";
    let script = "return io.read('*l')";
    let e = EvaluationBuilder::new(script, input.as_bytes()).build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn lmb_read_number(bencher: &mut Bencher) {
    let input = "1";
    let script = "return io.read('*n')";
    let e = EvaluationBuilder::new(script, input.as_bytes()).build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn lmb_read_unicode(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lmb'):read_unicode(1)";
    let e = EvaluationBuilder::new(script, input.as_bytes()).build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn read_from_buf_reader(bencher: &mut Bencher) {
    let mut r = BufReader::new(Cursor::new("1"));
    bencher.iter(|| {
        let mut buf = vec![0; 1];
        let _ = r.read(&mut buf);
    });
}

benchmark_group!(
    evaluation,
    lmb_evaluate,
    mlua_call,
    mlua_eval,
    mlua_sandbox_eval
);
benchmark_group!(
    read,
    lmb_read_all,
    lmb_read_line,
    lmb_read_number,
    lmb_read_unicode,
    read_from_buf_reader,
);
benchmark_group!(store, lmb_default_store, lmb_no_store, lmb_update);
benchmark_main!(evaluation, read, store);
