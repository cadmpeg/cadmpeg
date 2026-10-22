# FCStd coverage matrix

The primary profile is ZIP-packaged `SchemaVersion=4`, `FileVersion=1`. Each row requires an
independently authored, redistribution-cleared public fixture and the listed machine assertion.
Schema 4/file 1 is the one synthesis target. Schemas 2 and 3 are preserving-write profiles: a
retained document graph writes back the band it declares, and nothing regenerates one.

| Gate | Fixture classes                                                                                                         | Machine assertion                                                                                                                                                                                                                                |
| ---- | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| L0   | stored, deflated, ZIP64, data descriptor; GUI/headless; thumbnail present/absent                                        | detection identifies FCStd markers; inspection reports versions, kind, metadata, entry framing, and preview; the physical ledger closes                                                                                                          |
| L1   | core and extension objects; embedded and external assets; unknown property; string table and element map present/absent | every object/property/asset has stable identity and owner; links retain order; every logical ledger closes; unsupported bands are named                                                                                                          |
| L2   | analytic, Bezier, NURBS, trimmed, offset, swept, revolved, degenerate; text and binary B-rep; mesh and points           | every required carrier is typed; native values and canonical units agree; representative evaluations meet declared tolerance                                                                                                                     |
| L3   | solid, sheet, wire, compound, compsolid, multi-shell, void, seam, degenerate, non-manifold                              | topology is connected and validates; placements apply once; persistent names resolve only when carried                                                                                                                                           |
| L4   | attached sketch, dimensional parameters, additive feature, subtractive or dress-up feature                              | ordered history, dependencies, feature operands, state, and saved-result cross-links are complete                                                                                                                                                |
| L5   | every carrier and topology branch in the generated census; object/body/face/edge/point appearance                       | census has no unknown required carrier; exact topology validates without healing; appearance ownership and precedence validate                                                                                                                   |
| L6   | all Sketcher geometry/constraint branches; expressions; spreadsheet; core Part and PartDesign operations                | constraint, expression, cell, and operation graphs are typed and valid; the recomputed design census matches every projected object; design-domain loss report is empty                                                                          |
| L7   | groups, parts, nested links, link arrays, external links, assemblies, joints                                            | components and occurrences have distinct identity; local/world placements, persistent operands, missing externals, and cycles validate                                                                                                           |
| L8   | GUI state, TechDraw, annotations, embedded files, Mesh, Points, FEM, CAM, Python-backed and extension data              | presentation/drawing graphs validate; every application record is typed or named-retained; no payload executes; all physical/logical ledgers close                                                                                               |
| L9   | every manifested primary-envelope fixture; source-less parametric document; supported and unsupported target selections | writes are deterministic and decode to the same semantic fingerprint; typed edits survive; every named entry survives by identity and digest; source-less output is accepted by FreeCAD; unsupported targets and unsafe nested edits are refused |

Every gate also includes deterministic malformed-input cases for truncation, invalid counts and
indices, duplicate identities, missing owners, resource limits, and unsupported layout dispatch.

The checked-in [`freecad_fcstd-profile.json`](freecad_fcstd-profile.json) is generated from the
manifested corpus rather than edited by hand. Regenerate it from the repository root with:

```sh
cargo run -p cadmpeg-codec-freecad --bin fcstd-profile -- \
  corpus/freecad_fcstd/fixtures corpus/manifest.toml \
  docs/formats/freecad_fcstd-profile.json
```

Generation fails if a fixture digest or filename differs from the manifest. The profile decodes
every fixture twice, hashes canonical CADIR, runs neutral and native validation, rejects blocking
losses, verifies exact byte coverage, writes each fixture twice, decodes the result, compares its
semantic fingerprint, applies a typed edit, verifies named-entry survival, exercises source-less
generation and target refusal, and evaluates the cumulative ladder assertions. A failed
row remains failed in the artifact; the generator never promotes a score from filenames alone.

Independent application acceptance is checked after cadmpeg writes the manifested fixtures. Set
`CADMPEG_FCSTD_INPUT_DIR` to those outputs and `CADMPEG_FCSTD_OUTPUT_DIR` to a scratch directory,
then execute `tools/validate_fcstd_interop.py` inside `FreeCADCmd`. The validator fails when any
document is refused. Set `CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE=1` for fixtures expected to recompute,
save, and reopen without application-specific external assets.

## Design projection boundaries

Neutral sketch profiles are derived from active explicit coincident endpoint constraints. They do
not reproduce the cached shape's wires or the result of application recomputation. Unconstrained
equal or nearby endpoint coordinates remain separate; no decoder tolerance creates connectivity.
Non-construction entities participate in chains, including independently closed curves as single
entity profiles. Constraint branches and overlapping unconstrained entities do not acquire an
arbitrary continuation. Native geometry, constraints, cached topology, and operation selectors
remain independent retained records.

For deterministic neutral serialization, the lowest unused geometry ordinal seeds a chain with its
native direction. A unique explicit continuation extends each end, reversing an entity use when
its matching end requires that direction. Disconnected chains retain seed order. Closed chains
stop when no unused entity remains in the component. This ordering is a CADIR convention; it does
not claim persisted profile identity or producer wire traversal order. Full cached-wire-to-sketch
entity correspondence is not projected.

Non-container neutral feature dependencies include declared object dependencies and non-hidden link
runtime operands, without name or source-order filtering. Dependencies are resolved before stable
ordinal assignment. Known runtime and attachment-extension hidden-scope overrides apply in addition
to hidden link runtime names. Expression-engine dependencies resolve local object-name selectors and
unambiguous `<<Label>>` selectors, excluding quoted strings and external-document selectors. This
owner resolution does not evaluate formulas or claim complete expression-language coverage; unresolved
and external expressions remain native. PartDesign bodies keep membership as parent/child structure. Source declaration
order breaks ties between independently ready features. Cyclic native relations remain native and
use the existing blocking history diagnostic.

External cache association first uses a valid persisted export index, then a current mapped key,
and finally a unique historical alias. An out-of-range index or ambiguous historical alias leaves
the solved cached geometry unbound and reports `sketch.external-reference-unresolved`. Other
cache records and live external links remain available. Historical alias collisions alone do not
refuse a cache whose current keys are distinct.

## Implementation checklist

Apply this boundary to every phase:

- Commit no analysis-only checkout file, fixture, excerpt, path, source name, symbol name,
  citation, or evidence statement.
- Transcribe no implementation or test structure. State format rules in cadmpeg terminology and
  derive original tests from repository-owned rules and authorized fixtures.
- Keep specifications limited to settled byte semantics and invariants; place only genuine layout
  unknowns in the open-items file.
- Keep every contribution self-contained and reproducible after all analysis-only resources are
  removed.

## Pcurve location identity

Pcurve candidates match by surface identity and exact composed transform. A nonzero
transform difference, including a sub-`1.0e-12` difference, does not match. Competing candidates with
one native location-table index select the last serialized representation. Competing candidates
with distinct location-table indices retain their native records but omit the neutral pcurve with
`pcurve.location-ambiguous`; matrix equality does not prove native location equality.
This conservative case includes compound-location aliases: neutral projection does not yet compare
the native datum/power chains. The affected coedge retains its edge incidence but has no neutral
pcurve. The loss has blocking severity.

## Product metadata projection

Neutral part-number selection uses the runtime, not the validity or nonemptiness of competing
carriers. `App::Part`, `Assembly::AssemblyObject`, and `Assembly::AssemblyLink` select `Id`; an
absent, empty, or invalid `Id` leaves the neutral field unset. Their custom `PartNumber` string
remains a named BOM property. Other product runtimes project a valid nonempty `PartNumber` string
by the CADIR metadata convention; a same-named `Id` has no neutral part-number meaning. All native
properties remain independently retained. This convention does not prescribe a FreeCAD BOM
workbench's choice of columns.

Neutral link-occurrence transforms compose the local affine placements and scales before
`linked_prototype`; their separate neutral scale vector is unity. Native records retain the scalar
and per-element scale carriers. This keeps nonuniform scale in its source position relative to
rotations and translations.

## Rectangular-trimmed face support

A face on a rectangular-trimmed surface binds to the unrestricted basis surface. Nested outer trims
are removed from that face's support chain, and its pcurves use the basis UV chart. This preserves
evaluation at and outside the declared rectangle without clamping or changing pcurve coordinates.
The bounded subset constructions and the native trim bounds remain retained independently.

## Topology appearance association

Face materials and face, edge, and vertex colors bind through the saved shape's ordered topology
indices. The mapping does not require an element map and does not use the number of persistent
names as the topology count. Each index can target all neutral occurrences of that source
subelement. Missing indices withhold the override; they do not compact the remaining indices.

An indexed list whose length differs from the transferred topology count remains native and emits
`appearance.topology-color-count-mismatch`. An empty color list supplies no override. Empty material
lists remain count mismatches when a shape has faces. Duplicate `Shape` properties or payloads are
malformed; no source-order choice associates a view provider with one of them.
