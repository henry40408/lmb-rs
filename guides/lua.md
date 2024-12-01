# Lua Guide with Lmb

## Language Variant and Version

Lmb currently uses Luau from Roblox.

```lua
assert('Luau 0.653' == _G._VERSION, 'expect ' .. _G._VERSION) -- Luau version
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
assert(not m.store.a)
```

### Put

Insert or update the value in the store.

> [!WARNING]
> The value will be overwritten if it's present.

```lua
local m = require('@lmb')
m.store.b = 1
assert(1 == m.store.b)
```

### Update

The function accepts three arguments:

1. Keys
2. Function to update the values. It should return the new values. If an error is thrown, such as manually calling `error("something went wrong")`, the values will NOT be updated.
3. (Optional) Default values that will be passed as the first argument of the update function if any value is absent. Defaults to `nil` when omitted.

```lua
local m = require('@lmb')

local function do_update()
  return m.store:update({ 'c' }, function(values)
    local c = table.unpack(values)
    assert(tonumber(c), 'c is not a number')
    return table.pack(c + 1)
  end, { 1 })
end

assert(not m.store.c)

m.store.c = 1
assert(1 == m.store.c)
assert(2 == do_update()[1])
assert(2 == m.store.c)

m.store.c = 'not_a_number'
assert('not_a_number' == m.store.c)

local ok, err = pcall(do_update)
assert(not ok)
assert(string.find(tostring(err), 'c is not a number'))

assert('not_a_number' == m.store.c)
```

The following is a classic example of a transaction:

```lua
local m = require('@lmb')

local function transfer(amount)
  return m.store:update({ 'alice', 'bob' }, function(values)
    local alice, bob = table.unpack(values)
    if alice < amount then
      error('insufficient fund')
    end
    return table.pack(alice - amount, bob + amount)
  end, { 0, 0 })
end

m.store.alice = 50
m.store.bob = 50

local ok, err = pcall(function() return transfer(100) end) -- insufficient fund
assert(not ok)
assert(string.find(tostring(err), 'insufficient fund'))

assert(m.store.alice == 50)
assert(m.store.bob == 50)

m.store.alice = 100
m.store.bob = 0

local ok, res = pcall(function() return transfer(100) end) -- successful transfer
assert(ok)
local alice, bob = table.unpack(res)
assert(alice == 0)
assert(bob == 100)

assert(m.store.alice == 0)
assert(m.store.bob == 100)
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

assert(crypto:base64_encode(' ')    == 'IA==')
assert(crypto:base64_decode('IA==') == ' ')
assert(crypto:crc32('')  == '0')
assert(crypto:md5('')    == 'd41d8cd98f00b204e9800998ecf8427e')
assert(crypto:sha1('')   == 'da39a3ee5e6b4b0d3255bfef95601890afd80709')
assert(crypto:sha256('') == 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
assert(crypto:sha384('') == '38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b')
assert(crypto:sha512('') == 'cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e')
assert(crypto:hmac('sha1', '', 'secret') == '25af6174a0fcecc4d346680a72b7ce644b9a88e8')
assert(crypto:hmac('sha256', '', 'secret') == 'f9e66e179b6747ae54108f82f8ade8b3c25d76fd30afde6c395822c530196169')
assert(crypto:hmac('sha384', '', 'secret') == 'b818f4664d0826b102b72cf2a687f558368f2152b15b83a7f389e48c335fc455282c61e97335dae370bac31a8196772d')
assert(crypto:hmac('sha512', '', 'secret') == 'b0e9650c5faf9cd8ae02276671545424104589b3656731ec193b25d01b07561c27637c2d4d68389d6cf5007a8632c26ec89ba80a01c77a6cdd389ec28db43901')

local data = ''
local key = '01234567'

local encrypted = crypto:encrypt(data, 'des-ecb', key)
assert(encrypted == '08bb5db6b37c06d7')
local decrypted = crypto:decrypt(encrypted, 'des-ecb', key)
assert(decrypted == '')

local iv = '01234567'

local encrypted = crypto:encrypt(data, 'des-cbc', key, iv)
assert(encrypted == 'b9b77ae196c39d7a')
local decrypted = crypto:decrypt(encrypted, 'des-cbc', key, iv)
assert(decrypted == '')

local key = '0123456701234567'
local iv = '0123456701234567'

local encrypted = crypto:encrypt(data, 'aes-cbc', key, iv)
assert(encrypted == 'f71257f2ffa6808961efb09ad82c2abd')
local decrypted = crypto:decrypt(encrypted, 'aes-cbc', key, iv)
assert(decrypted == '')

```
