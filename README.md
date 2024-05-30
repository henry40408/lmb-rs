# lam

> lam is a Lua function runner

[![Casual Maintenance Intended](https://casuallymaintained.tech/badge.svg)](https://casuallymaintained.tech/)
![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/henry40408/lam/.github%2Fworkflows%2Fworkflow.yaml)
![GitHub](https://img.shields.io/github/license/henry40408/lam)
[![codecov](https://codecov.io/gh/henry40408/lam/graph/badge.svg?token=O7WLYVEX0E)](https://codecov.io/gh/henry40408/lam)

## Features

- Evaluate a Lua script.
- Handle HTTP requests via a Lua script.
- Schedule a Lua script with cron.

## Installation

### Prerequisites

- Rust ≥ 1.78.0

```bash
git clone https://github.com/henry40408/lam
cd lam
cargo install --path . --locked
```

## Usage

Find some examples:

```bash
lam example ls
```

Evaluate an example:

```bash
lam example eval --name hello
```

Evaluate Lua script:

```bash
$ lam eval --file lua-examples/hello.lua
hello, world!
```

Handle HTTP requests with single script:

```bash
$ lam serve --file lua-examples/echo.lua
(another shell session) $ curl -X POST http://localhost:3000 -d $'hello'
hello
```

## License

MIT
