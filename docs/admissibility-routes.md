# Admissibility routes

Successful decode is not a valid IR. Decoder and export gates use documented
subsets of [`Check`](../crates/cadmpeg-ir/src/report.rs); final document
validation remains `validate_neutral` (+ fidelity + native at the application
boundary). Accept/reject fixtures are in
[admissibility-fixtures.md](admissibility-fixtures.md).

## Shared core

`DRAFT_CORE_CHECKS` =

- `Identity`
- `ReferentialIntegrity`
- `NativeLinks`
- `LoopClosure`
- `CoedgePairing`
- `ShellTopology`
- `WireTopology`
- `CarrierReachability`
- `ParameterDomain`
- `Bounds`
- `GeometricConsistency`

API: `cadmpeg_ir::validate::admit::{admit, admit_with_annotations,
admit_with_additional_native_identities, filter_checks}` and the route
constants below.

## Per-route subsets

| route                                            | constant / policy                           | notes                                                                                                                                   |
| ------------------------------------------------ | ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Rhino draft                                      | `RHINO_DRAFT_CHECKS` = core + `Annotations` | `ArenaOrder` excluded (pre-finalize)                                                                                                    |
| Rhino instance                                   | `RHINO_INSTANCE_CHECKS` = core              | `ArenaOrder` excluded; no mid-expansion finalize                                                                                        |
| CATIA admission                                  | `CATIA_ADMISSION_CHECKS` = core             | finalize first; pending native identities via admit helper                                                                              |
| IGES rejection                                   | full `validate_neutral`                     | classified keep-full: `DRAFT_CORE_CHECKS` ≡ full errors on all 78 IGES golden fixtures                                                  |
| SLDPRT export precondition (`writer` input)      | full `validate_neutral`                     | keep-full: refusal depends on non-core Checks (`Counts`, …); `SLDPRT_EXPORT_PRECONDITION_CHECKS` is the documented draft/topology floor |
| SLDPRT writer postcondition (after bake/prepare) | full `validate_neutral`                     | postcondition on the writer's own transform; not replaced                                                                               |

Codecs not listed run no decode/export admission gate; their documents are
checked only at the application boundary (`validate_neutral` + fidelity +
native validators).

## Native passthrough arenas

A semantic writer refuses a native arena it cannot preserve, so an arena the
writer does not regenerate is admitted by name. The IGES semantic writer admits
`quarantined_directory_records` and `quarantined_parameter_records` as
passthrough arenas. It regenerates neither, because a record that failed typing
has no fields to write, and a non-empty one charges
`iges/writer.passthrough-omitted` naming the arena and its record count.
Byte-exact replay is a separate path and is unaffected: it copies the retained
source image, which holds every quarantined card.

## Application composition

`cadmpeg check` / convert validation =

1. `validate_neutral` (or `validate_neutral_with_source_fidelity`)
2. plus every registered native validator whose namespace is present

Encoders count rows with `CadIr::census` / `entity_census`, not validation.
