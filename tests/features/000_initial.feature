Feature: A simple application to run Lua functions

  Scenario: Evaulate a lua file
    Given a lua script
      | script        | result |
      |               |        |
      | return 1+1    | 2      |
      | return 'a'..1 | a1     |
      | local M = require('@lam'); return M._VERSION | 0.1.0 |
    When it is evaluated
    Then it should return result

    Given a lua script
      | script            | result |
      | while true do end |        |
    When the timeout is set to 1 second
    And it is evaluated
    Then it should return result
