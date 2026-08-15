# Admissibility accept/reject fixtures

Frozen before any production gate switches onto a narrow `Check` subset.
Rejected candidates are load-bearing: decoder rollback and writer refusal are
driven by rejections, not only by accepts.

## Shared IR builders

`crates/cadmpeg-ir/src/validate/admissibility_freeze.rs`:

| fixture                   | outcome | driving check          |
| ------------------------- | ------- | ---------------------- |
| `accepted_empty`          | accept  | —                      |
| `rejected_missing_point`  | reject  | `ReferentialIntegrity` |
| `rejected_missing_region` | reject  | `ReferentialIntegrity` |

## Per-route pins (current gates, full validator)

| route                       | site                                    | accept                        | reject                                                                          |
| --------------------------- | --------------------------------------- | ----------------------------- | ------------------------------------------------------------------------------- |
| Rhino draft                 | `decode.rs` candidate commit            | empty IR; ArenaOrder stripped | missing point / missing region                                                  |
| Rhino instance              | `decode.rs` expansion                   | empty IR                      | missing point / missing region                                                  |
| CATIA admission             | `assemble::neutral_model_is_admissible` | empty IR; pending unknowns    | shell → missing region (`assemble` tests)                                       |
| IGES rejection              | `reader::reject_invalid_semantic_ir`    | valid decode IR               | missing point (`semantic_decode_barrier_rejects_invalid_cadir`)                 |
| SLDPRT export precondition  | `writer.rs:50`                          | normalized decode IR          | face → missing surface (`semantic_writer_rejects_invalid_ir_without_panicking`) |
| SLDPRT writer postcondition | `writer.rs:79`                          | after bake/prepare            | same transform must preserve validity (full validator kept)                     |

Codec-local tests named above remain the route-level freeze. Shared builders pin
the cross-route Check outcomes used by the shadow differential.
