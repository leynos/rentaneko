# Migrating to composed `RUSTFLAGS`

This note covers the change to how the `test`, `lint`, `typecheck` and `build`
Make targets handle `RUSTFLAGS` and target selection. It applies to the next
pre-1.0 minor release.

## What changed

- The targets keep an exported `RUSTFLAGS` and append the build standard's
  flags after it. Earlier recipes replaced the exported value.
- The targets add `-Zthreads=8`, and the `mold` linker flag only when both the
  build host and the compilation target are Linux.
- A `--target` in `CARGO_FLAGS`, `TEST_FLAGS` or `CLIPPY_FLAGS` now stops the
  targets with an error, because Make cannot read the target from those
  variables to decide whether `mold` applies.

## What callers need to do

1. Move target selection from `CARGO_FLAGS`, `TEST_FLAGS` or `CLIPPY_FLAGS` to
   `CARGO_BUILD_TARGET`. Use `host-tuple` to mean the host's own triple.

   ```sh
   # Before
   make test TEST_FLAGS="--target aarch64-apple-darwin"
   # After
   make test CARGO_BUILD_TARGET=aarch64-apple-darwin
   ```

2. Remove from an exported `RUSTFLAGS` any flag that duplicated the standard
   (`-Zthreads=8` or the `mold` linker flag), unless the duplicate is wanted.
3. Leave `make coverage` and `make release` as they are. Neither keeps the
   standard flags, and their behaviour is unchanged.

See the [user guide](users-guide.md) for the full flag rules.
