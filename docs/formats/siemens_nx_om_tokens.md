<!-- Generated from crates/cadmpeg-codec-nx/src/om_tokens.rs. Do not edit by hand; run `cargo test -p cadmpeg-codec-nx`. -->

# NX object-model serialization vocabulary

The NX OM class registry is a run of length-framed `UGS::` names discovered in the stream, so there is no fixed record-class set. The fixed vocabulary is the literals the OM decode keys on and the closed set of numeric-expression unit tokens.

## Structural anchors

| Name | Literal | Description |
|---|---|---|
| root_marker | `\x04\x01\x0eNX ` | root entity marker anchoring an accepted section's first record |
| host_globals | `hostglobalvariables` | section gate: numeric expressions decode only when present |
| class_name_prefix | `UGS::` | registered class-definition name prefix in the type run |
| number_prefix | `(Number [` | numeric-expression prefix immediately before the unit token |

## Numeric-expression units

| Token | Unit | Description |
|---|---|---|
| `mm` | Millimeter | canonical model length in millimeters |
| `degrees` | Degree | angular value in degrees |
