# Story 3 of workshop-2 (the allowlist-mutation example-mapping session,
# 2026-08-23), co-authored by Casey and Mez. Design record:
# ARCHITECTURE.md decision 28 (rules R1-R5 and rulings Q1-Q3 of the
# session; board archived at docs/mappings/2026-08-23-allowlist-mutation/).
#
# EXECUTABLE — every scenario below is bound to step definitions in
# tests/cucumber.rs and runs on `cargo test`. See features/README.md for
# the runner, and for the standing rule: a scenario without a step is a
# gap, not a decoration.

Feature: An actor's guest list changes while it runs
  As an operator of a long-running actor
  I need to admit new endpoints and revoke old ones without restarting
  anything — with a revocation biting immediately, even mid-exchange —
  so that trust can be granted, rotated, and withdrawn at the speed of
  operations rather than the speed of redeployment

  Scenario: A running actor warms up to a new peer
    Given a running actor whose allowlist is empty
    When its operator admits a new endpoint
    Then a request from that endpoint is delivered and acknowledged
    And the actor was never restarted

  # Binding note for the cucumber-rs errand: "the very next delivery"
  # means the next delivery PROCESSED AFTER THE REVOCATION RESOLVES. On
  # the Kameo runtime, a delivery already queued in the actor's mailbox
  # ahead of the revoke is processed under the old admission (decision
  # 26's processing-time shape; KameoRuntime::revoke's doc records it).
  # Steps must send after awaiting the revoke, as the pinning tests do —
  # a step written from the scenario text alone will pass on LocalRuntime
  # and flake on Kameo.
  Scenario: Revocation bites on the very next delivery
    Given a running actor that admits two endpoints
    And a conversation in progress with each
    When its operator revokes the second endpoint
    Then the next delivery from the revoked endpoint is denied
    And the denial is observed at home
    And the first endpoint's conversation is untouched

  Scenario: Revocation fails the live exchange at once
    Given an exchange awaiting the peer's turn
    When the operator revokes that peer's endpoint
    Then the exchange fails at once, well before any deadline
    And the failure names the revocation as its cause

  Scenario: The conversation survives the revocation
    Given an exchange that failed because its peer was revoked
    When the operator admits that endpoint again
    Then a fresh exchange opens in the same conversation

  Scenario: A key rotates without the actor missing a beat
    Given a running actor that admits the old key's endpoint
    When its operator admits the new key's endpoint
    And its operator revokes the old key's endpoint
    Then a command under the old key is denied and the denial observed
    And a command under the new key is delivered
    And the actor was never restarted

  Scenario: A second revocation changes nothing
    Given an exchange that failed because its peer was revoked
    When the operator revokes that same endpoint again
    Then no further failure is reported to anyone

  Scenario: Revoking a stranger changes nothing
    Given a running actor that admits one endpoint
    When its operator revokes an endpoint that was never admitted
    Then the admitted endpoint's deliveries continue unaffected

  Scenario: Revoking the last guest restores silence
    Given a running actor that admits one endpoint
    When its operator revokes that endpoint
    Then the actor receives nothing from anyone
