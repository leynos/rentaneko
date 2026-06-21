# Documentation contents

[Documentation contents](contents.md) is the index for Rentaneko's
documentation set.

## Project guides

- [Terms of reference](terms-of-reference.md) defines the problem space, users,
  scope, constraints, and open questions for the Rentaneko prototype.
- [Rentaneko prototype design](rentaneko-design.md) specifies the walking
  skeleton for the Podbot 3.3.1 token-writer spike.
- [ADR 001: Use Simulacat Core for the Octocrab spike](adr-001-use-simulacat-core-for-octocrab-spike.md)
  records the backend decision, fail-fast compatibility checkpoint, and
  process-contract requirements.
- [Roadmap](roadmap.md) sequences the prototype work and deferred extensions.
- [User guide](users-guide.md) explains how to use the generated project and
  its public build and test commands.
- [Developer guide](developers-guide.md) explains the local workflow and
  implementation tooling for contributors.
- [Repository layout](repository-layout.md) explains the generated project's
  top-level files, directories, and ownership boundaries.
- [Documentation style guide](documentation-style-guide.md) defines the
  spelling, structure, Markdown, Architecture Decision Record (ADR), Request
  for Comments (RFC), and roadmap conventions used by this documentation set.

## Rust reference material

- [Reliable testing in Rust via dependency injection](reliable-testing-in-rust-via-dependency-injection.md)
  explains how to keep tests deterministic by injecting environment, clock,
  filesystem, and other external dependencies.
- [Rust doctest Don't Repeat Yourself guide](rust-doctest-dry-guide.md)
  explains how to write maintainable, executable Rust documentation examples.
- [`rstest-bdd` user's guide](rstest-bdd-users-guide.md) preserves the
  upstream `rstest-bdd` v0.5.0 user guide as local reference material.
- [Rust testing with `rstest` fixtures](rust-testing-with-rstest-fixtures.md)
  explains fixture-based, parameterized, and asynchronous testing with `rstest`.

## Engineering practice

- [Complexity antipatterns and refactoring strategies](complexity-antipatterns-and-refactoring-strategies.md)
  explains cognitive complexity, the bumpy-road antipattern, and refactoring
  approaches for maintainable code.
- [Scripting standards](scripting-standards.md) explains the preferred Python
  scripting stack, command execution patterns, and test expectations for helper
  scripts.
