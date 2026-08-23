# Story 4 of workshop-2 (the same-endpoint fan-out example-mapping
# session, 2026-08-23), co-authored by Casey and Mez. Design record:
# ARCHITECTURE.md decision 29 (rules R1-R5 and rulings Q1-Q4 of the
# session; board archived at docs/mappings/2026-08-23-fanout/).
#
# R1 (same-endpoint only) is enforced by the type of the fan-out call —
# it takes one endpoint and a list of names, so a mixed-endpoint batch
# is unrepresentable and needs no runtime scenario.
#
# EXECUTABLE — every scenario below is bound to step definitions in
# tests/cucumber.rs and runs on `cargo test`. See features/README.md for
# the runner, and for the standing rule: a scenario without a step is a
# gap, not a decoration.

Feature: One send opens many conversations
  As a controller with several actors at one endpoint
  I need to open work with all of them in a single send — receiving one
  delivery receipt for the batch and N ordinary conversations back —
  so that starting a fleet of workers costs one wire round-trip, without
  inventing any new kind of conversation

  Scenario: One send reaches three actors
    Given three actors at one endpoint, each admitting the controller
    When the controller opens work with all three in one send
    Then three ordinary conversations proceed
    And each concludes on its own schedule

  Scenario: The batch receipt settles every wait at once
    Given a fan-out opening awaiting its delivery receipt
    When the one receipt for the batch arrives
    Then every conversation's receipt wait settles
    And no deadline was consumed doing it

  Scenario: A denied sibling is silence, and the rest proceed
    Given three actors at one endpoint, one of which never admitted the controller
    When the controller opens work with all three in one send
    Then two conversations proceed
    And the denial is observed at the actors' home
    And the controller's exchange with the denying actor fails by its turn deadline

  Scenario: An absent sibling discloses nothing
    Given a batch naming one actor that exists and one that does not
    When the controller opens work with both in one send
    Then the receipt still arrives
    And the existing actor's conversation proceeds
    And the absent actor's exchange fails by its turn deadline

  Scenario: A fanned conversation can be revoked alone
    Given three conversations born of one fan-out send
    When one sibling's operator revokes the controller's endpoint
    Then that exchange fails at once, naming the revocation
    And the other two conversations are untouched
