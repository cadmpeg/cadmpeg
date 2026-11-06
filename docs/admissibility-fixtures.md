# Validation fixtures

The shared builders in `crates/cadmpeg-test-support/src/admissibility.rs`
construct an empty document and documents with a missing point or region.
They exercise neutral validation and reference integrity. Decoder part tests
check that independently readable records survive damage. Writer tests check
that output reports omissions and reductions.
