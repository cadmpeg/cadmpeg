# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

When an item is resolved, delete it in the same change that writes the answer into
the specification. Do not keep a Resolved part. Neutral projection limits are
listed in [the coverage document](freecad_fcstd-coverage.md).



## 12. Validation

### QA-01. Native geometry equivalence and profile assertions

**Question.** Which independent geometry and design invariants must the native
acceptance helper and generated profile verify before they report a successful
semantic write?

**Known.** `tools/validate_fcstd_interop.py` accepts a document returned by
`openDocument`. Its optional recompute/save/reopen path compares object names
and types, without checking the recompute result, object error states, or
geometry. With `CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE=1`, the WR-01 two-box output
reports `accepted=true`, `native_resave_accepted=true`, and two objects although
one shape is null.

The profile in `crates/cadmpeg-codec-freecad/src/bin/fcstd_profile.rs` checks
required topology kind names at L3, a nonzero appearance binding count at L5,
and required constraint and operation labels at L6. Its L9 assertions compare
cadmpeg fingerprints, a document-label edit, and entry name/digest equality.
Neutral and native validation also run, but these checks do not establish every
independent semantic guarantee named in the coverage matrix.

**Need.** Compare native results with the original or an authored oracle for
shape nullness, solid and face counts, volume, placement, recompute results,
and error states. Check appearance ownership and precedence across the required
target classes, and compare the recomputed design census and design edits.
Align each profile assertion with the guarantee that its coverage row names.

**Conflict.** The helper certifies the missing-shape output. Self-round-trip
and record-presence assertions can pass while native geometry or a required
semantic comparison is absent.

**Note.** Add the missing-shape case to the acceptance criteria of
[issue #243](https://github.com/cadmpeg/cadmpeg/issues/243). Keep document-open,
self-round-trip, and native semantic-equivalence results distinct. The published
headline remains L5; the L9 write extras do not establish full application support.
