#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use serde_json::{json, Value};
    use tempfile::NamedTempFile;

    #[test]
    fn check_stdin() {
        let mut cmd = Command::cargo_bin("lam").unwrap();
        cmd.write_stdin("ret true");
        cmd.args(["--no-color", "check", "--file", "-"]);
        cmd.assert().success().stderr(
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
    fn eval_file() {
        let mut cmd = Command::cargo_bin("lam").unwrap();
        cmd.args(["eval", "--file", "lua-examples/01-hello.lua"]);
        #[cfg(not(windows))]
        cmd.assert().success().stdout("hello, world!\n");
        #[cfg(windows)]
        cmd.assert().success().stdout("hello, world!\r\n");
    }

    #[test]
    fn eval_json_output() {
        let mut cmd = Command::cargo_bin("lam").unwrap();
        cmd.args([
            "--json",
            "eval",
            "--file",
            "lua-examples/08-return-table.lua",
        ]);
        cmd.assert().success();
        let s = String::from_utf8(cmd.output().unwrap().stdout).unwrap();
        let parsed: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(json!(true), *parsed.get("a").unwrap());
        assert_eq!(json!(1.23f64), *parsed.get("b").unwrap());
        assert_eq!(json!("hello"), *parsed.get("c").unwrap());
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
}
