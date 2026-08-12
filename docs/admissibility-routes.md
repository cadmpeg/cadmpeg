# Admissibility routes (Phase 5)

Successful decode is not a valid IR. Decoder and export gates use documented
subsets of [`Check`](../crates/cadmpeg-ir/src/report.rs); final document
validation remains `validate_neutral` (+ fidelity + native at the application
boundary).

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

| route | constant / policy | notes |
| --- | --- | --- |
| Rhino draft | `RHINO_DRAFT_CHECKS` = core + `Annotations` | `ArenaOrder` excluded (pre-finalize) |
| Rhino instance | `RHINO_INSTANCE_CHECKS` = core | `ArenaOrder` excluded; no mid-expansion finalize |
| CATIA admission | `CATIA_ADMISSION_CHECKS` = core | finalize first; pending native identities via admit helper |
| IGES rejection | full `validate_neutral` | classified: subset error outcomes ≡ full on freeze fixtures and golden corpus; keep full barrier |
| SLDPRT export precondition (`writer` input) | `SLDPRT_EXPORT_PRECONDITION_CHECKS` = core | refuses write on error-severity findings in the set |
| SLDPRT writer postcondition (after bake/prepare) | full `validate_neutral` | postcondition on the writer's own transform; not replaced |

## Application composition

`cadmpeg validate` / convert validation =

1. `validate_neutral` (or `validate_neutral_with_source_fidelity`)
2. plus every registered native validator whose namespace is present

Encoders count rows with `CadIr::census` / `entity_census`, not validation.
