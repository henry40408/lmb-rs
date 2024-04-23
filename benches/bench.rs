#![allow(clippy::unwrap_used)]

use bencher::{benchmark_group, benchmark_main, Bencher};
use lam::EvalBuilder;
use mlua::prelude::*;
use std::io::{BufReader, Cursor, Read as _};

static SCRIPT: &str = "return true";

/// evaluation

fn lam_evaluate(bencher: &mut Bencher) {
    let e = EvalBuilder::new(SCRIPT.into()).build();
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

fn lam_no_store(bencher: &mut Bencher) {
    let e = EvalBuilder::new(SCRIPT.into()).build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn lam_default_store(bencher: &mut Bencher) {
    let e = EvalBuilder::new(SCRIPT.into()).with_default_store().build();
    bencher.iter(|| e.evaluate().unwrap());
}

/// read

fn lam_read_all(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*a')";
    let mut e = EvalBuilder::new(script.into())
        .with_input(input.as_bytes())
        .build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn lam_read_line(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*l')";
    let mut e = EvalBuilder::new(script.into())
        .with_input(input.as_bytes())
        .build();
    bencher.iter(|| {
        e.set_input(&b"0"[..]);
        e.evaluate().unwrap()
    });
}

fn lam_read_number(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*n')";
    let mut e = EvalBuilder::new(script.into())
        .with_input(input.as_bytes())
        .build();
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
    lam_evaluate,
    mlua_call,
    mlua_eval,
    mlua_sandbox_eval
);
benchmark_group!(
    read,
    lam_read_all,
    lam_read_line,
    lam_read_number,
    read_from_buf_reader,
);
benchmark_group!(store, lam_default_store, lam_no_store);
benchmark_main!(evaluation, read, store);
