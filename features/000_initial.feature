Feature: A simple application to run Lua functions

  Scenario: Evaulate a lua file
    Given a lua script
      | script                 | result |
      | print('l')             |        |
      | return 1+1             | 2      |
      | return 'a' .. 1        | a1     |
      | print('l'); return 2+2 | 4      |
    When it is evaluated
    Then it should return result

    Given a lua script
      | script            | result |
      | while true do end |        |
    When the timeout is set to 1 second
    And it is evaluated
    Then it should return result
