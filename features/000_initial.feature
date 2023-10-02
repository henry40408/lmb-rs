Feature: A simple application to run Lua functions

  Scenario: Evaulate a lua file
    Given a lua script
      | script                 | result |
      | print('hello, world!') |        |
      | return 1+1             | 2      |
      | return 'a' .. 1        | a1     |
    When a user evaulates it
    Then they should have result
