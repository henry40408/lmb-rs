# Getting Started with Lmb

## Language Variant and Version

Lmb currently uses Luau 0.625 from Roblox.

Luau is a Lua 5.1 language with gradual typing and ergonomic additions.

## Security

To enhance security, Lmb enables the sandbox mode of Luau. For details, please refer to the [Luau documentation](https://luau-lang.org/sandbox).

## Hello, World

First things first: Hello, World!

Save the following file as "hello.lua".

```lua
print("hello, world!")
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
print("hello, world!")

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stdout
io.write("standard output")

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.read
io.read("*a")
io.read("*l")
io.read("*n")

-- https://www.lua.org/manual/5.1/manual.html#pdf-io.stderr
io.stderr:write("standard error")
```

## Store

Lmb supports a key-value store backed by SQLite. The data can be read, written, and updated using the following APIs:

### Get

`nil` will be returned when the value is absent.

```lua
local m = require('@lmb')
print(m:get('a')) -- nil
```

### Put

Insert or update the value in the store.

> [!WARNING]
> The value will be overwritten if it's present.

```lua
local m = require('@lmb')
m:put('b', 1)
print(m:get('b')) -- 1
```

### Update

The function accepts three arguments:

1. Key
2. Function to update the value. It should return the new value. If any error is thrown, such as manually calling `error("something went wrong")`, the value will not be updated.
3. Default value: will be passed as the first argument of the update function when the value is absent.
   In the following example, if "c" is absent, "c" will be set to 2 eventually.

```lua
local m = require("@lmb")

local function do_update()
	m:update("c", function(c)
		assert(tonumber(c), "c is not a number")
		return c + 1
	end, 1)
end

print(m:get("c")) -- nil

m:put("c", 1)
print(m:get("c")) -- 1
do_update()
print(m:get("c")) -- 2

m:put("c", "not_a_number")
print(m:get("c")) -- not_a_number
do_update() -- no error will be thrown
print(m:get("c")) -- not_a_number
```

#### When Should `update` Be Used?

When an atomic operation on the value is required because the `update` function wraps the operation in a database transaction.

## Initialize Store

An in-memory SQLite database will be created and migrated when not specified. However, any changes will be lost when the program terminates.

To make the store persistent, users need to designate a store path and properly migrate. Lmb supports automatic migration. To enable it, set the `LMB_STORE_MIGRATE` environment variable or use `--run-migrations` when evaluating the script.

The following example will create and migrate a store called `db.sqlite3` in the current path:

```sh
$ lmb --store-path db.sqlite3 --run-migrations eval --file lua-examples/store.lua
1
```

# HTTP

Lmb is able to send HTTP requests. It provides a function called `fetch`, whose signature is similar to the [Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch) from JavaScript. The following example sends a GET request to https://httpbin.org/headers with the header `I-Am: A teapot`:

```lua
-- Be aware: The function is in another module called `@lmb/http`.
local http = require("@lmb/http")

local res = http:fetch("https://httpbin.org/headers", {
	method = "GET",
	headers = {
		["I-Am"] = "A teapot",
	},
})
print(res.headers["content-type"][1]) -- application/json
print(res:json()["headers"]["I-Am"]) -- A teapot
```

## Why Refer to the JavaScript Fetch API?

I have used JavaScript and Node.js for a decade, and the Fetch API is the method I am most familiar with for sending HTTP requests.

# JSON

> TBC

# Crypto

> TBC
