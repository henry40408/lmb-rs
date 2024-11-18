# Lua Guide with Lmb

## Language Variant and Version

Lmb currently uses Luau from Roblox.

```lua
assert('Luau 0.635' == _G._VERSION, 'expect ' .. _G._VERSION) -- Luau version
```

Luau is a Lua 5.1 language with gradual typing and ergonomic additions.

For all packages and functions provided by Luau, please refer to the [Luau documentation](https://luau-lang.org/library).

## Security

To enhance security, Lmb enables the sandbox mode of Luau. For details, please refer to the [Luau documentation](https://luau-lang.org/sandbox).

## Hello, World

First things first: Hello, World!

Save the following file as "hello.lua".

```lua
print('hello, world!')
```

Run the script:

```sh
$ lmb eval --file hello.lua
hello, world!
```

## I/O Library

According to the [Luau documentation](https://luau-lang.org/sandbox#library):

> The `io` library has been removed entirely, as it gives access to files and allows running processes.

However, because it's common to print and read something in daily use, Lmb implements the following functions/methods:

```lua
-- https://www.lua.org/manual/5.1/manual.html#pdf-print
print('hello, world!')

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stdout
io.write('standard output')

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.read
io.read('*a')
io.read('*l')
io.read('*n')

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stderr
io.stderr:write('standard error')
```

## Store

Lmb supports a key-value store backed by SQLite. The data can be read, written, and updated using the following APIs:

### Get

`nil` will be returned when the value is absent.

```lua
local m = require('@lmb')
assert(not m:get('a'))
```

### Put

Insert or update the value in the store.

> [!WARNING]
> The value will be overwritten if it's present.

```lua
local m = require('@lmb')
assert(1 == m:put('b', 1))
assert(1 == m:get('b'))
```

## Initialize Store

An in-memory SQLite database will be created and migrated when not specified. However, any changes will be lost when the program terminates.

To make the store persistent, users need to designate a store path and properly migrate. Lmb supports automatic migration. To enable it, set the `LMB_STORE_MIGRATE` environment variable or use `--run-migrations` when evaluating the script.

The following example will create and migrate a store called `db.sqlite3` in the current path:

```sh
$ lmb --store-path db.sqlite3 --run-migrations eval --file lua-examples/store.lua
1
```

## HTTP `@lmb/http`

Lmb is able to send HTTP requests. It provides a function called `fetch`, whose signature is similar to the [Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch) from JavaScript. The following example sends a GET request to <https://httpbin.org/headers> with the header `I-Am: A teapot`:

```lua
local http = require('@lmb/http')

local res = http:fetch('https://httpbin.org/headers', {
  method = 'GET',
  headers = {
    ['I-Am'] = 'A teapot',
  },
})
assert('application/json' == res.headers['content-type'][1])
assert('A teapot' == res:json()['headers']['I-Am'])
```

### Why Refer to the JavaScript Fetch API?

I have used JavaScript and Node.js for a decade, and the Fetch API is the method
I am most familiar with for sending HTTP requests.

## JSON `@lmb/json`

JSON is a common format used to send HTTP requests. Lmb supports both encoding and decoding JSON data:

```lua
local json = require('@lmb/json')
assert('{"bool":true,"num":1.23,"str":"string"}' == json:encode({ bool = true, num = 1.23, str = 'string' }))
assert('[true,1.23,"string"]' == json:encode({ true, 1.23, 'string' }))

-- Caveat: Explicit keys will be dropped when encoding to JSON if the table is mixed with explicit and implicit keys.
assert('[true,"string"]' == json:encode({ true, num = 1.23, 'string' }))

local decoded = json:decode('{"bool":true,"num":1.23,"str":"string"}')
assert(true == decoded.bool)
assert(1.23 == decoded.num)
assert('string' == decoded.str)

local decoded = json:decode('[true,1.23,"string"]')
assert(true == decoded[1])
assert(1.23 == decoded[2])
assert('string' == decoded[3])

-- https://github.com/rxi/json.lua/issues/19
local expected = '{"key":[{},{},{}]}'
local actual = json:encode(json:decode(expected))
assert(actual == expected)
```

Send an HTTP request with a JSON request body:

```lua
local http = require('@lmb/http')
local json = require('@lmb/json')

local res = http:fetch('https://httpbin.org/post', {
	method = 'POST',
	body = json:encode({ foo = 'bar' }),
})
assert('{"foo":"bar"}' == res:json().data)
```

## Crypto `@lmb/crypto`

When receiving webhook events from another service, e.g. [GitHub](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries), it's secure to validate them before processing. Lmb provides several cryptography functions to meet this need:

- SHA256
- HMAC-SHA256
- (Contributions are welcome if more algorithms are needed)

```lua
local crypto = require('@lmb/crypto')
assert('2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824' == crypto:sha256('hello'))
assert('88aab3ede8d3adf94d26ab90d3bafd4a2083070c3bcce9c014ee04a443847c0b' == crypto:hmac('sha256', 'hello', 'secret'))
```
