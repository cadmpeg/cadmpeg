# CADIR wire version 11

CADIR documents, decode sidecars, and command reports use wire version 11.
Readers accept version 11. Earlier versions are refused without migration.
The required `scope` object declares whether the model and design classes are
held. A populated class is in scope, and an empty in-scope arena means none was
found or authored.

A request for more content than the input holds produces the available document
and a `depth.not-delivered` loss. F3D design and NX attachment failures reduce
scope when no part of the affected class can be found. A damaged or unsupported
part is given up with a `record.unreadable` Error; independently readable parts
are decoded. Its bytes are retained in fidelity for its `unknowns` stub.

Writers produce the content held in the document and report reductions and
omissions. Synthesis uses neutral records and the writer's typed native records.
Patch and replay require an empty `SourceFidelity.unrepresented`, the union of
the retained records' `untyped` class marks. Otherwise a writer that can reuse
source selects synthesis with `SourceUnrepresented`. A declined patch selects
synthesis with `PatchDeclined`.

FreeCAD synthesis writes an FCStd archive from CADIR, including OCCT BRep text.
`Feature.needs_recompute` records stale evaluation state. Writers preserve the
format's recompute flag where available and otherwise charge
`export:evaluation.stale`.

`convert` writes available content when checking reports errors, a sidecar is
stale, or geometry output is empty. `--reject-errors` and `--reject-empty` opt
into refusal for check errors and empty geometry; `--allow-errors` and
`--allow-empty` are removed. `--container-only` permits
CADIR output; another output is refused with `container_only_output`.
`--reject-lossy=decode` judges the current native decode or CADIR parse. A
restored sidecar describes the earlier decode. `--reject-lossy-floor` selects
the minimum rejected severity and defaults to `info`.

New losses are `record.unreadable` (Error), `sidecar.stale` (Warning),
`export:record.residual-unavailable` (Warning), `export:value.quantized` (Info),
and `export:output.empty` (Warning). Codec-specific losses for given-up parts
are replaced by `record.unreadable`; the omitted native-record loss identifies
omitted typed records.

See [the document contract](../document-contract.md) for codec, writer, and
editor requirements.
