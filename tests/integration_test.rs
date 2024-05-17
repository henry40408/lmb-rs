use assert_cmd::Command;
use serde_json::{json, Value};
use tempfile::NamedTempFile;

#[test]
fn check_stdin() {
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.write_stdin("ret true");
    cmd.args(["--no-color", "check", "--file", "-"]);
    cmd.assert().failure().stderr(
        r#"Error: leftover token
   ,-[(stdin):1:1]
 1 |ret true
   | `-- leftover token
"#,
    );
}

#[test]
fn eval_stdin() {
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.write_stdin("return 1+1");
    cmd.args(["eval", "--file", "-"]);
    cmd.assert().success().stdout("2");
}

#[test]
fn eval_example() {
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.write_stdin("1949\n");
    cmd.args(["eval", "--file", "lua-examples/algebra.lua"]);
    cmd.assert().success().stdout("3798601");
}

#[test]
fn eval_file() {
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.args(["eval", "--file", "lua-examples/hello.lua"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("hello, world!"));
}

#[test]
fn eval_json_output() {
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.args(["--json", "eval", "--file", "lua-examples/return-table.lua"]);
    cmd.assert().success();
    let s = String::from_utf8(cmd.output().unwrap().stdout).unwrap();
    let value: Value = serde_json::from_str(&s).unwrap();
    let expected = json!({
        "bool": true,
        "num": 1.23,
        "str": "hello",
    });
    assert_eq!(expected, value);
}

#[test]
fn eval_store_migrate() {
    let store = NamedTempFile::new().unwrap();
    let store_path = store.path().to_string_lossy().to_string();
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.write_stdin("return true");
    cmd.args([
        "eval",
        "--file",
        "-",
        "--store-path",
        &store_path,
        "--run-migrations",
    ]);
    cmd.assert().success();
}

#[test]
fn store_migrate() {
    let store = NamedTempFile::new().unwrap();
    let store_path = store.path().to_string_lossy().to_string();
    let mut cmd = Command::cargo_bin("lam").unwrap();
    cmd.args(["store", "migrate", "--store-path", &store_path]);
    cmd.assert().success();
}
