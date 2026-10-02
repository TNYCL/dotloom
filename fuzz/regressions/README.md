# Fuzz regressions

Inputs that once crashed a fuzz target, one directory per target
(`fuzz/regressions/<target>/`). `crates/cli/tests/fuzz_replay.rs` replays every file
here through the same harness as the cargo-fuzz targets in every test run, so a fixed
crash stays fixed.

No crash has been found so far (nightly runs, `docs/status.md`). When one is:

1. download the `fuzz-crash-<target>` artifact of the failing nightly run (or take
   the input from `target/fuzz-failures/` after a local `fuzz_replay` failure);
2. fix the bug and add a focused unit test where the bug lives;
3. copy the input to `fuzz/regressions/<target>/<short-description>` and commit it
   with the fix.
