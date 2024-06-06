use std::time::Duration;

use assert_cmd::Command;
use assert_fs::NamedTempFile;
use serde_json::{json, Value};

#[test]
fn check_stdin_syntax_error() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("ret true");
    cmd.args(["--no-color", "check", "--file", "-"]);
    cmd.assert().failure().stderr(
        r#"Error: leftover token
   ,-["-":1:1]
 1 |ret true
   | `-- leftover token
"#,
    );
}

#[test]
fn check_stdin_tokenizer_error() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("return !true");
    cmd.args(["--no-color", "check", "--file", "-"]);
    cmd.assert().failure().stderr(
        r#"Error: unexpected character !
   ,-["-":1:8]
 1 |return !true
   |       `- unexpected character !
"#,
    );
}

#[test]
fn eval_file() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["eval", "--file", "lua-examples/hello.lua"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("hello, world!"));
}

#[test]
fn eval_json_output() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.args(["--json", "example", "eval", "--name", "return-table"]);
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
fn eval_stdin() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.write_stdin("return 1+1");
    cmd.args(["eval", "--file", "-"]);
    cmd.assert().success().stdout("2");
}

#[test]
fn eval_stdin_syntax_error() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("return !true");
    cmd.args(["--no-color", "eval", "--file", "-"]);
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("Unexpected"));
}

#[test]
fn eval_store_migrate() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("return true");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "eval",
        "--file",
        "-",
    ]);
    cmd.assert().success();
}

#[test]
fn example_cat() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["example", "cat", "--name", "hello"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("hello"));
}

#[test]
fn example_eval() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.write_stdin("1949\n");
    cmd.args(["example", "eval", "--name", "algebra"]);
    cmd.assert().success().stdout("3798601");
}

#[test]
fn example_list() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["example", "list"]);
    cmd.assert().success();
}

#[test]
fn example_serve() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args([
        "example",
        "serve",
        "--bind",
        "127.0.0.1:3000",
        "--name",
        "hello",
    ]);
    cmd.timeout(Duration::from_secs(1));
    cmd.assert().stdout(predicates::str::contains("serving"));
}

#[test]
fn list_themes() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["list-themes"]);
    cmd.assert().success();
}

#[test]
fn schedule() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("require('@lmb'):put('a', 1); return true");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "schedule",
        "--cron",
        "* * * * * *",
        "--initial-run",
        "--file",
        "-",
    ]);
    cmd.timeout(Duration::from_millis(1_100));
    cmd.assert().stderr("");

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "get",
        "--name",
        "a",
    ]);
    cmd.assert().stdout("1");
}

#[test]
fn serve() {
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args([
        "serve",
        "--bind",
        "127.0.0.1:3001",
        "--file",
        "lua-examples/hello.lua",
    ]);
    cmd.timeout(Duration::from_secs(1));
    cmd.assert().stdout(predicates::str::contains("serving"));
}

#[test]
fn store_delete() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.write_stdin("1");
    cmd.env("RUST_LOG", "error");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "put",
        "--name",
        "a",
        "--value",
        "-",
    ]);
    cmd.assert().success().stdout("1");

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "delete",
        "--name",
        "a",
    ]);
    cmd.assert().success().stdout("1");
}

#[test]
fn store_get() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "get",
        "--name",
        "a",
    ]);
    cmd.assert().success().stdout("null");
}

#[test]
fn store_get_list_put() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.write_stdin("1");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "put",
        "--name",
        "a",
        "--value",
        "-",
    ]);
    cmd.assert().success().stdout("1");

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "list",
    ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.env("RUST_LOG", "error");
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "get",
        "--name",
        "a",
    ]);
    cmd.assert().success().stdout("1");
}

#[test]
fn store_list() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args([
        "--store-path",
        &store_path,
        "--run-migrations",
        "store",
        "list",
    ]);
    cmd.assert().success();
}

#[test]
fn store_migrate() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["--store-path", &store_path, "store", "migrate"]);
    cmd.assert().success();
}

#[test]
fn store_version() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    let mut cmd = Command::cargo_bin("lmb").unwrap();
    cmd.args(["--store-path", &store_path, "store", "version"]);
    cmd.assert().success();
}
