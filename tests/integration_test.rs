#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use serde_json::{json, Value};

    #[test]
    fn eval_stdin() {
        let mut cmd = Command::cargo_bin("lam").unwrap();
        cmd.write_stdin("return 1+1");
        cmd.args(["eval"]);
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
            "eval",
            "--file",
            "lua-examples/08-return-table.lua",
            "--output-format",
            "json",
        ]);
        cmd.assert().success();
        let s = String::from_utf8(cmd.output().unwrap().stdout).unwrap();
        let parsed: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(json!(true), *parsed.get("a").unwrap());
        assert_eq!(json!(1.23f64), *parsed.get("b").unwrap());
        assert_eq!(json!("hello"), *parsed.get("c").unwrap());
    }
}
