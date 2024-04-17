use lam::EvalBuilder;
use mlua::prelude::*;

use bencher::{benchmark_group, benchmark_main, Bencher};

fn baseline(bencher: &mut Bencher) {
    let e = EvalBuilder::new(&b""[..], "return true").build();
    bencher.iter(|| e.evaluate());
}

fn original(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load("return true").eval::<()>());
}

benchmark_group!(evaluation, original, baseline);
benchmark_main!(evaluation);
