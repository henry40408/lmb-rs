use lam::EvalBuilder;
use mlua::prelude::*;

use bencher::{benchmark_group, benchmark_main, Bencher};

static SCRIPT: &str = "return true";

fn baseline(bencher: &mut Bencher) {
    let e = EvalBuilder::new(&b""[..], SCRIPT).build();
    bencher.iter(|| e.evaluate());
}

fn load_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn call_function(bencher: &mut Bencher) {
    let vm = Lua::new();
    let f = vm.load(SCRIPT).into_function().expect("");
    bencher.iter(|| f.call::<_, bool>(()));
}

benchmark_group!(evaluation, baseline, call_function, load_eval);
benchmark_main!(evaluation);
