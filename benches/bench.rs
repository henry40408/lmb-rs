#![allow(clippy::unwrap_used)]

use lam::{EvalBuilder, LamStore};
use mlua::prelude::*;

use bencher::{benchmark_group, benchmark_main, Bencher};

static SCRIPT: &str = "return true";

fn lam_evaluate(bencher: &mut Bencher) {
    let e = EvalBuilder::new(&b""[..], SCRIPT).build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn mlua_load_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn mlua_call_function(bencher: &mut Bencher) {
    let vm = Lua::new();
    let f = vm.load(SCRIPT).into_function().unwrap();
    bencher.iter(|| f.call::<_, bool>(()).unwrap());
}

fn lam_no_store(bencher: &mut Bencher) {
    bencher.iter(|| {
        let e = EvalBuilder::new(&b""[..], SCRIPT).build();
        e.evaluate().unwrap()
    });
}

fn lam_default_store(bencher: &mut Bencher) {
    bencher.iter(|| {
        let store = LamStore::default();
        let e = EvalBuilder::new(&b""[..], SCRIPT).set_store(store).build();
        e.evaluate().unwrap()
    });
}

fn lam_read_all(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*a')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap()
    });
}

fn lam_read_line(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*l')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap()
    });
}

fn lam_read_number(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*n')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap()
    });
}

benchmark_group!(evaluation, lam_evaluate, mlua_load_eval, mlua_call_function);
benchmark_group!(read, lam_read_all, lam_read_line, lam_read_number);
benchmark_group!(store, lam_default_store, lam_no_store);
benchmark_main!(evaluation, read, store);
