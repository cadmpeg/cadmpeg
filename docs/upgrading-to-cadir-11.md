<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# Upgrading from CADIR 10 to CADIR 11

cadmpeg reads and writes CADIR version 11 (`"ir_version": "11"`). Releases up to 0.6.12 wrote version 10. [`cadir-v11.md`](cadir-v11.md) states the version 11 wire, and [`cad-ir.md`](cad-ir.md) gives the document layout.

## Stored files

cadmpeg refuses a CADIR document, a decode sidecar (`<stem>.fidelity.json`), and a JSON command report of version 10. The error names the version and the main differences. No tool converts a version 10 file to version 11. Keep the source CAD files and decode them again:

```sh
cadmpeg dump bracket.f3d -o bracket.cadir.json
```

Keep the decode sidecar beside its `.cadir.json`. A write back to FreeCAD or SolidWorks reads the sidecar to find the records you edited, and charges `export:edit.unverified` where the sidecar is missing. Where the sidecar does not read or does not match its document, the document loads without it and the load reports `sidecar.stale`.

## What a saved document holds

A saved `.cadir.json` holds the four neutral layers `geometry`, `model`, `design`, and `process`, and the `native` records of the source format.

- The feature history is in `design`. A decoder also holds the history arenas that version 10 wrote under `model`, in memory, and a conversion from the source file reads them. A saved document holds the `design` records that restate them. Content that a decoder states only in those arenas is absent from a saved document, for example a physics joint that a USD write from the source file writes. A CADIR write reports each such arena as `export/arena.omitted`.
- A SolidWorks or Fusion write from a document read from CADIR JSON writes an unedited document from its retained source and refuses an edited one. A document that the library decodes from the source file and edits in the same process writes its edits as before.
- A document holds no `document_local_sha256` and no `*_local_sha256` source attribute. The decode sidecar records the content digest and the per-format digests.

## Document layout

| Version 10 | Version 11 |
|---|---|
| `decode_depth` | `scope`: `{ "model": bool, "design": bool, "process": bool }` |
| `units`: `{ "length": … }` | `units` with `length`, `angle`, `mass`, and `time` |
| — | `fragment`, always written |
| `model.bodies`, `regions`, `shells`, `faces`, `loops`, `coedges`, `edges`, `vertices`, `points`, `surfaces`, `curves`, `subds`, `pcurves`, `procedural_surfaces`, `procedural_curves` | the same arenas under `geometry` |
| `model.features` | `design.features`; the feature tree is `design.containers`, each part's or assembly's design is a `design.scopes` record, and stored states are `design.evaluations` |
| `model.parameters`, `configurations`, `spreadsheets`, `sketches`, `sketch_entities`, `sketch_constraints`, `spatial_sketches`, `spatial_sketch_entities`, `spatial_sketch_constraints` | the same arenas under `design` |
| `model.assembly_joints` | `Joint` and `Place` features in `design.features` |
| `model.feature_input_topologies`, `feature_result_topologies` | `design.evaluations` where the source stores the state |
| — | `process`: setups, operations, tools, programs, and toolpaths |

The other `model` arenas (`product_definitions`, `occurrences`, `tessellations`, `appearances`, `attributes`, `pmi`, `drawings`, and the presentation and physics arenas) stay in `model`. Their records change shape: [`cadir-v11-migration.md`](cadir-v11-migration.md) maps the record types to their version 11 form.

Every value follows one spelling rule (`cadir-v11.md` §2.5):

- An enum with data writes its variant under `kind`, and a payload that is not a record under `value`.
- A member with no value is omitted. A reader refuses `null`.
- Every list, map, and set is written, also when it is empty.
- A document holds one zero: a reader reads `-0` as `0`, and a writer writes `0.0`.

## Command line

- `cadmpeg query` names arenas by layer: `geometry.faces`, `design.features`, `model.occurrences`. Change `jq` paths the same way: `.model.faces` is `.geometry.faces`. [`jq-recipes.md`](jq-recipes.md) has the version 11 paths.
- `cadmpeg check` applies the validation rules of `cadir-v11.md` part 10. It can report errors on a decode that an earlier release passed. Findings of the design and process layers have `check` values `design` and `process`. In `check --json`, a finding always writes `related`, also when it is empty.
- `convert` writes the output when the check finds errors, and its report lists the findings. `--allow-errors` is removed. `--reject-errors` refuses the write (exit 1) when the check finds an error in what the output writes.
- A writer produces a file and reports losses for many documents it refused before. A script that used a refusal as its signal reads the losses, or passes `--reject-lossy=export`.
