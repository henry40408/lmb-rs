use cucumber::{gherkin::Step, given, then, when, World as _};
use lam::evaluate;

#[derive(Debug)]
struct Case {
    script: String,
    expected: String,
}

#[derive(cucumber::World, Debug, Default)]
struct World {
    cases: Vec<Case>,
    results: Vec<String>,
}

#[given(expr = "a lua script")]
fn give_a_lua_file(w: &mut World, step: &Step) {
    for row in step.table.as_ref().unwrap().rows.iter().skip(1) {
        let script = &row[0];
        let expected = &row[1];
        w.cases.push(Case {
            script: script.to_string(),
            expected: expected.to_string(),
        });
    }
}

#[when(expr = "a user evaulates it")]
fn user_evaluates_it(w: &mut World) {
    for case in &w.cases {
        w.results.push(evaluate(&case.script).unwrap());
    }
}

#[then(expr = "they should have result")]
fn should_have_result(w: &mut World) {
    for (idx, case) in w.cases.iter().enumerate() {
        let result = w.results.get(idx);
        assert_eq!(Some(&case.expected), result);
    }
}

#[tokio::main]
async fn main() {
    World::run("features/000_initial.feature").await;
}
