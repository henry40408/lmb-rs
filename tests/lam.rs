use std::{cell::RefCell, io::Cursor, rc::Rc};

use cucumber::{gherkin::Step, given, then, when, World as _};
use lam::{evaluate, Evaluation, EvaluationResult};

#[derive(Debug)]
struct Case {
    expected: String,
    input: String,
    script: String,
}

#[derive(cucumber::World, Debug, Default)]
struct World {
    cases: Vec<Case>,
    results: Vec<EvaluationResult>,
    timeout: Option<u64>,
}

#[given("a lua script")]
fn give_a_lua_file(w: &mut World, step: &Step) {
    for row in step.table.as_ref().unwrap().rows.iter().skip(1) {
        let script = &row[0];
        let expected = &row[1];
        let input = row.get(2).map(String::from).unwrap_or(String::new());
        w.cases.push(Case {
            script: script.to_string(),
            expected: expected.to_string(),
            input,
        });
    }
}

#[when("it is evaluated")]
fn user_evaluates_it(w: &mut World) {
    for case in &w.cases {
        let mut e = Evaluation {
            input: Rc::new(RefCell::new(Cursor::new(case.input.clone()))),
            script: case.script.clone(),
            timeout: w.timeout,
        };
        w.results.push(evaluate(&mut e).unwrap());
    }
}

#[when(expr = "the timeout is set to {int} second(s)")]
fn set_timeout(w: &mut World, secs: u64) {
    w.timeout = Some(secs);
}

#[then("it should return result")]
fn should_have_result(w: &mut World) {
    for (idx, case) in w.cases.iter().enumerate() {
        let result = w.results.get(idx);
        assert_eq!(case.expected, result.unwrap().result);
    }
}

#[tokio::main]
async fn main() {
    World::run("tests/features/000_initial.feature").await;
}
