# Story 2 of the workshop-2 example-mapping session (2026-08-19/20),
# co-authored by Casey and Mez. Design record: ARCHITECTURE.md decision 27
# (rules R6-R8 and rulings Q5/Q6 of the session; board archived at
# docs/mappings/2026-08-19-timeouts-disconnects/).
#
# EXECUTABLE — every scenario below is bound to step definitions in
# tests/cucumber.rs and runs on `cargo test`. See features/README.md for
# the runner, and for the standing rule: a scenario without a step is a
# gap, not a decoration.

Feature: A vanished peer fails loudly
  As an application embedding kamiroh in unattended, container-based tests
  I need a peer that is killed mid-exchange surfaced in seconds, as positive
  evidence — while an ordinary wire blip must not kill anything the
  glossary promised would survive it

  Scenario: The peer endpoint dies mid-exchange
    Given an exchange awaiting the peer's turn
    When the peer's endpoint is killed
    And the transport reports the peer's endpoint dead
    Then the exchange fails at once, well before any deadline
    And the waiting party is told the exchange failed

  Scenario: The conversation survives the death
    Given an exchange that failed because its peer's endpoint was killed
    When the peer returns under the same endpoint identity
    Then a fresh exchange opens in the same conversation

  Scenario: A wire blip is not a death
    Given an exchange awaiting the peer's turn
    When the connection drops and is re-established
    And the peer's turn arrives within the deadline
    Then the exchange continues in the same conversation

  Scenario: A conversation spans connections
    Given a conversation whose exchange completed over one connection
    When that connection is deliberately closed
    Then the next exchange in the same conversation travels a new connection
    And the receiving side routes it to the same actor

  Scenario: A silent death is caught by the backstop
    Given an exchange awaiting the peer's turn
    And the transport observes nothing unusual
    When the peer's process is frozen rather than killed
    Then the turn deadline elapses and the exchange fails with a timeout
