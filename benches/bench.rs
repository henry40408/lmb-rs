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
    let f = vm.load(SCRIPT).into_function().expect("");
    bencher.iter(|| f.call::<_, bool>(()));
}

benchmark_group!(evaluation, lam_baseline, mlua_load_eval, mlua_call_function);
benchmark_main!(evaluation);
