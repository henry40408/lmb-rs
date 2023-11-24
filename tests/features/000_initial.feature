Feature: A simple application to run Lua functions

  Scenario: Evaulate a Lua script
    Given a lua script
      | script        | result | input |
      |               |        |       |
      | return 1+1    | 2      |       |
      | return 'a'..1 | a1     |       |
      | local M = require('@lam'); return M._VERSION   | 0.1.0 |     |
    When it is evaluated
    Then it should return result

  Scenario: Evaluate a Lua script w/ timeout
    Given a lua script
      | script            | result |
      | while true do end |        |
    When the timeout is set to 1 second
    And it is evaluated
    Then it should return result

  Scenario: Evaluate a Lua script w/ lam module
    Given a lua script
      | script                                         | result | input |
      | local M = require('@lam'); return M.read('*a') | lam    | lam   |
      | local M = require('@lam'); return M.read(1)    | l      | lam   |
      | local M = require('@lam'); return M.read(3)    | l      | l     |
      | local M = require('@lam'); return M.read('*a') | 你好   | 你好  |
      | local M = require('@lam'); return M.read_unicode(1) | l    | lam  |
      | local M = require('@lam'); return M.read_unicode(2) | l    | l    |
      | local M = require('@lam'); return M.read_unicode(1) | 你   | 你好 |
      | local M = require('@lam'); return M.read_unicode(2) | 你   | 你   |
      | local M = require('@lam'); return M.read_unicode(2) | 你好 | 你好 |
    When it is evaluated
    Then it should return result

  Scenario: Evaluate Lua examples
    Given a filename of lua script
      | filename        | input  | expected |
      | 01-hello.lua    |        |          |
      | 02-input.lua    | lua    |          |
      | 03-algebra.lua  | 2      | 4        |
    When it is evaluated
    Then it should return result
