# Admissibility accept/reject fixtures

Frozen before any production gate switches onto a narrow `Check` subset.
Rejected candidates are load-bearing: decoder rollback and writer refusal are
driven by rejections, not only by accepts.

## Shared IR builders

`crates/cadmpeg-test-support/src/admissibility.rs`:

| fixture                   | outcome | driving check          |
| ------------------------- | ------- | ---------------------- |
| `accepted_empty`          | accept  | —                      |
| `rejected_missing_point`  | reject  | `ReferentialIntegrity` |
| `rejected_missing_region` | reject  | `ReferentialIntegrity` |

## Per-route pins (current gates, full validator)

| route                   | site                                    | accept                            | reject                                                                                                  |
| ----------------------- | --------------------------------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Rhino draft             | `decode.rs` candidate commit            | empty IR; ArenaOrder stripped     | missing point / missing region                                                                          |
| Rhino instance          | `decode.rs` expansion                   | empty IR                          | missing point / missing region                                                                          |
| CATIA admission         | `assemble::neutral_model_is_admissible` | empty IR; pending unknowns        | shell → missing region (`assemble` tests)                                                               |
| IGES projection check   | `reader::invalid_semantic_ir`           | valid decode IR: no loss          | missing point: one `projection.invalid` loss per finding                                                |
| SLDPRT written document | `writer::reduce::invalid_records`       | written cube: no `record.invalid` | vertex off its edges: `record.invalid` (`a_validation_error_in_the_written_document_is_record_invalid`) |

Codec-local tests named above remain the route-level freeze. Shared builders pin
the cross-route Check outcomes used by the shadow differential.
