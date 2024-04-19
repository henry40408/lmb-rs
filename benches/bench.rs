#![allow(clippy::unwrap_used)]

use lam::EvalBuilder;
use mlua::prelude::*;

use bencher::{benchmark_group, benchmark_main, Bencher};

static SCRIPT: &str = "return true";

fn lam_baseline(bencher: &mut Bencher) {
    let e = EvalBuilder::new(&b""[..], SCRIPT).build();
    bencher.iter(|| e.evaluate());
}

fn mlua_load_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn mlua_call_function(bencher: &mut Bencher) {
    let vm = Lua::new();
    let f = vm.load(SCRIPT).into_function().unwrap();
    bencher.iter(|| f.call::<_, bool>(()));
}

fn lam_read_all(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*a')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap();
    });
}

fn lam_read_line(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*l')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap();
    });
}

fn lam_read_number(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*n')";
    bencher.iter(|| {
        let e = EvalBuilder::new(input.as_bytes(), script).build();
        e.evaluate().unwrap();
    });
}

benchmark_group!(evaluation, lam_baseline, mlua_load_eval, mlua_call_function);
benchmark_group!(read, lam_read_all, lam_read_line, lam_read_number);
benchmark_main!(evaluation, read);
