# Story 1 of the workshop-2 example-mapping session (2026-08-19/20),
# co-authored by Casey and Mez. Design record: ARCHITECTURE.md decisions
# 22–25; deliberations: docs/briefs/ and the session board.
#
# EXECUTABLE — every scenario below is bound to step definitions in
# tests/cucumber.rs and runs on `cargo test`. See features/README.md for
# the runner, and for the standing rule: a scenario without a step is a
# gap, not a decoration.

Feature: A hung exchange fails loudly
  As an application embedding kamiroh in unattended, container-based tests
  I need an exchange whose peer has gone silent to become a loud, prompt,
  assertable failure — never a hang

  Background:
    Given every conversation surface is constructed with finite deadlines
    And each side's deadlines bound its own waiting only

  Scenario: A peer that never answers
    Given an exchange between two parties with a turn deadline
    And the exchange is awaiting the peer's turn
    When the deadline elapses with no turn arriving
    Then the exchange fails with a timeout
    And the waiting party is told the exchange failed
    And the conversation may open a new exchange

  Scenario: A slow but timely answer
    Given an exchange with a turn deadline
    When the peer's turn arrives before the deadline elapses
    Then the exchange continues as if no deadline existed

  Scenario: An ack that never comes
    Given a sent turn awaiting its delivery ack, with an ack deadline
    When the ack deadline elapses first
    Then the exchange fails with a timeout
    And the sender's party is told the exchange failed

  Scenario: A late turn after a failed exchange
    Given an exchange that already failed by timeout
    When the peer's answer finally arrives
    Then it is refused as no part of any exchange

  Scenario: The two sides converge on failure separately
    Given an exchange whose sender has failed it on an elapsed ack deadline
    And a peer that believes the exchange is alive
    When the peer's own turn deadline elapses with no further turn arriving
    Then the peer's side of the exchange fails with a timeout
    And no failure message has crossed the wire

  Scenario: A send the transport refuses
    Given an exchange whose next turn is handed to the transport
    When the transport refuses to carry it
    Then the exchange fails at once, well before any deadline
    And the party is told the exchange failed
    And the conversation may open a new exchange

  Scenario: A denied delivery is observable at home
    Given an actor whose allowlist does not admit a sender's endpoint
    When the unadmitted sender's delivery arrives
    Then the delivery is denied and the sender learns nothing
    And the denial is observable on the receiving side
