Feature: Octocrab consumes the Simulacat Core installation-token route

  Background:
    Given a managed Simulacat Core seeded with installation 2000 for app 1
    And an App-authenticated octocrab client pointed at the simulator

  Scenario: Acquire an installation token from the managed Simulacat Core
    When the client requests an installation token for installation 2000
    Then the token equals "FAKE_GITHUB_TOKEN"

  Scenario: An unknown installation is rejected
    When the client requests an installation token for installation 9999
    Then octocrab reports that installation 9999 is unknown
