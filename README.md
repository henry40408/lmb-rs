# lam

> Lua function runner

## Features

- Evaluate Lua code
- Manage functions and result of each execution

## Installation

> TODO

## Usage

Evaluate Lua script:

```
$ ./lam eval --file lua-examples/01-hello.lua
```

Handle HTTP requests with single script:

```
$ ./lam serve --file lua-examples/04-echo.lua
(in another terminal) $ curl -X POST http://localhost:3000 -d $'hello'
hello
```

## License

MIT
