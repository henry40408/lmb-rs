# lam

> Lua function runner

[![Casual Maintenance Intended](https://casuallymaintained.tech/badge.svg)](https://casuallymaintained.tech/)
![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/henry40408/lam/.github%2Fworkflows%2Fworkflow.yaml)
![GitHub](https://img.shields.io/github/license/henry40408/lam)
[![codecov](https://codecov.io/gh/henry40408/lam/graph/badge.svg?token=O7WLYVEX0E)](https://codecov.io/gh/henry40408/lam)

## Features

- Evaluate Lua script.
- Handle HTTP requests via Lua script.

## Installation

### Prerequisites

- Rust >= 1.78.0

```bash
git clone https://github.com/henry40408/lam
cd lam
cargo build --release
```

## Usage

Find some examples:

```bash
$ ./target/release/lam example ls
```

Evaluate an example:

```bash
$ ./target/release/lam example eval --name hello
```

Evaluate Lua script:

```bash
$ ./target/release/lam eval --file lua-examples/hello.lua
hello, world!
```

Handle HTTP requests with single script:

```bash
$ ./target/release/lam serve --file lua-examples/echo.lua
(another shell session) $ curl -X POST http://localhost:3000 -d $'hello'
hello
```

## License

MIT
