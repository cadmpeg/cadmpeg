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

The L3 topology assertion requires the topology family matrix and error-free neutral validation.
The L5 appearance assertion checks body, face, edge, and vertex target classes. Each binding must
reference an existing asset, belong to the source object's exact-shape property, and carry the
required face, edge, or vertex override precedence. Duplicate effective assignments fail. The L6
census assertion independently compares every projected feature's object, identity, semantic family,
and post-processing state with its native census record. A separate edit assertion requires an
actual dimensional parameter edit to change the selected typed neutral operation; a document label
or a retained native parameter map does not satisfy it. The independent recomputed-design assertion
also requires a SHA-bound native report for every design fixture and matches each projected owner
and runtime type with the recomputed native census. A persisted projection census alone does not
satisfy that assertion. Without native evidence, L6 remains failed.

Independent application acceptance is separate from CADIR self-round-trip. Set
`CADMPEG_FCSTD_INPUT_DIR` to writer outputs, `CADMPEG_FCSTD_REFERENCE_DIR` to independently authored
originals, and `CADMPEG_FCSTD_OUTPUT_DIR` to a scratch directory. Run
`tools/validate_fcstd_interop.py` inside `FreeCADCmd` with
`CADMPEG_FCSTD_REQUIRE_SEMANTIC_EQUIVALENCE=1` and `CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE=1`.
The validator compares each original and output by object identity and type, dependencies,
serializable design properties, ordered sketch constraint kinds and operands, auxiliary native
shapes, physical and appearance material properties, shape nullness and validity, topology counts,
volume, area, length, bounds, placement, mesh topology, and point data. Unsupported required core
design properties fail. Other unsupported application properties are named with their runtime and
Python types in `uncompared_properties`; semantic acceptance covers the stated geometry and design
scope, not those retained application payloads. It forces native design recomputation, checks
object error states and the recompute result, and repeats the comparison after save/reopen. A zero
recomputed-object count is a successful no-op. An edited output is compared after recomputation;
an unchanged output must also match before recomputation. A missing saved shape therefore fails
even when the native application could regenerate it.

Set `CADMPEG_FCSTD_REQUIRE_APPEARANCE=1` to initialize native GUI view providers and compare their
shape materials, indexed colors, line/point styles, visibility, and display mode. Native GUI
comparison requires a display server or `QT_QPA_PLATFORM=offscreen` with a supported Qt backend.
`CADMPEG_FCSTD_EXPECTATIONS` can name a JSON object keyed by output filename. Each entry can select
its original with `reference` and supply `edits`, an array of `{object, property, value}` records.
The edits apply to the independent original before recomputation; they do not copy expected values
from the written output.

The JSON report separates `document_opened`, `semantic_equivalence_accepted`, and
`native_resave_accepted`; self-resave alone is not independent semantic equivalence.
`CADMPEG_FCSTD_REPORT` writes that report to a file. Native command wrappers can suppress a script's
exit status, so callers must require the report and an empty `failures` array. Set
`CADMPEG_FCSTD_NATIVE_REPORT` when generating the profile to consume the native report. L9 requires
a successful independent native comparison for every written fixture, bound to both original and
written SHA-256 digests. The separate source-less assertion consumes `source_less` from that same
report: `source_sha256`, `native_recompute_accepted`, `dimensions_mm`, `solids`, `faces`, and
`volume_mm3`. It requires the generated bytes, dimensions `[12.5, 7, 3]` mm, one solid, six faces,
and volume 262.5 cubic mm. Missing native evidence leaves the corresponding assertions failed;
deterministic byte or CADIR checks do not substitute for it.

The native regression workflow runs the authored restoration, input-edit, source-less shape,
mesh-placement, spline-domain, and filleted STEP cases with FreeCAD 1.0.0. It checks ordinary recomputation of
edited and newly constructed objects separately from the validator's forced design recomputation.
Run the same gate locally after building the CLI and examples:

```sh
cargo build -q -p cadmpeg --no-default-features --features fcstd,step
cargo build -q -p cadmpeg-codec-freecad --examples
python3 scripts/run-fcstd-interop.py \
  --cadmpeg target/debug/cadmpeg \
  --writer-example target/debug/examples/interop_outputs \
  --source-less-example target/debug/examples/source_less \
  --freecad-command 'FreeCADCmd' \
  --work-dir /tmp/fcstd-interop
```

The work directory contains native reports, measurement and geometry-comparison reports,
originals, writer outputs, native resaves, and per-stage logs. STEP import checks require matching
face counts, area, volume, and bounds for AP203 edition 1, AP214, and AP242 edition 3. Mesh cases
compare neutral world vertices with native occurrence transforms because body measurements do not
cover application tessellations. These regression cases do not substitute for the complete
manifested-corpus evidence required by the higher profile assertions.

The spline case compares measured edge length and periodic-face area with native geometry using
the measurement report's numeric error and model uncertainty allowances. Its original file has
both a rounded 3D spline endpoint and a periodic pcurve that crosses its fundamental knot seam.

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

Pcurve candidates match by surface identity and the fully composed native location chain. The
last serialized matching representation supplies the pcurve references and range. Compound aliases
match after adjacent datum-power reduction; distinct elementary datums do not match even when their
matrices are equal. The edge placement and representation location compose before comparison with
the placed face support.

If no native chain matches but a candidate has an equal matrix, the affected coedge retains its
edge incidence and omits its neutral pcurve with blocking `pcurve.location-ambiguous`. Native records
remain retained. The native record model does not distinguish an empty compound location from an
elementary location with no factors. Projection conservatively treats each such table row as a
distinct datum; it does not infer an empty chain from an identity matrix.

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
The bounded subset constructions and the native trim bounds remain retained independently. Each
bypassed subset carrier has the support geometry role so exporters do not emit it as a separate
free surface.

## Topology appearance association

Face materials and face, edge, and vertex colors bind through the saved shape's ordered topology
indices. The mapping does not require an element map and does not use the number of persistent
names as the topology count. Each index can target all neutral occurrences of that source
subelement. Missing indices withhold the override; they do not compact the remaining indices.

An indexed list whose length differs from the transferred topology count remains native and emits
`appearance.topology-color-count-mismatch`. An empty color list supplies no override. Empty material
lists remain count mismatches when a shape has faces. Duplicate `Shape` properties or payloads are
malformed; no source-order choice associates a view provider with one of them.

Multi-material lists whose length equals the solid count still require the face count to match.
The decoder does not infer per-solid materials from that equality, expand identical entries into a
uniform material, or assign a prefix of an unmatched list. Such lists remain native and keep the
count-mismatch warning. The warning identifies the absent face association; it does not determine
why the saved list and shape have different counts.

An admitted edge or vertex color array replaces the corresponding default color bindings on its
indexed targets. The effective array appearances retain the default line width or point size.
An empty or mismatched array leaves those default bindings in place.

## Exact location identity

Topology indexing, vertex and edge sharing, and representation association use ordered elementary
datum identities and signed powers. Equal affine matrices do not merge distinct datum identities. Adjacent equal datums
combine their powers, and a zero sum cancels that pair. Records with no retained factors keep a
distinct table identity, including records whose affine matrix is the identity matrix.

One interned chain has one canonical transform evaluated from its reduced datum powers. A canceled
chain evaluates to the exact identity transform. Topology labels and ordered source indices use
native chain identity; curve and surface placement caches use their canonical affine matrices.
Repeated shape/chain states are visited once per root or source-index target, preserving first
encounter order.

Canonical location words admit at most 65,536 terms. Location-table reconstruction admits the
larger of 16,777,216 term visits and 1,024 visits per location row, including intermediate powers
and compositions. Compound powers use repeated squaring. Word expansion, work exhaustion, or
signed 64-bit exponent overflow returns a local bound outcome; the caller withholds the affected
topology and reports a loss. Topology admission also bounds visits, composition calls and term scans, cached products, and total
interned terms by the larger of 16,777,216 and 1,024 times the combined location and shape row count.
Admission completes before topology mutation. A bound withholds that payload's entire neutral
topology with blocking `topology.location-identity-bounded`; later payloads continue. Allocation
failures remain decode errors. Canonical identities are
derived state and do not change the native record format.

## Application mesh placement

Neutral application meshes carry model-space vertices. The projection applies owning placements,
nested geometric containers, direct mesh links, expanded and collapsed link arrays, and links to
geometric containers. Each linked use has a distinct tessellation identity and an occurrence path
in its source association. Parent transforms are not carried a second time by a neutral body.
Native mesh bytes and placement properties remain retained for preserving writes.

A link to a link, a link to an ordinary document group, or a link that selects mesh subelements
has no complete mesh projection. The affected use is withheld with blocking
`geometry.mesh-placement-not-transferred`; independent source meshes and supported uses remain.
This boundary does not change point-cloud transform handling.

Mesh occurrence expansion admits at least 65,536 visits, or 64 times the combined native product
and neutral occurrence row count, whichever is larger. Mesh-less targets and child subtrees are
removed from the expansion index. Local work exhaustion withholds the entire current use and
all subsequent uses with blocking `geometry.mesh-placement-bounded`. Independent source meshes
and earlier complete uses remain. Cloned mesh storage, loss text, and mesh entities are admitted
before retention.

## Primitive placement

Primitive frames use the saved resolved `Placement` for map modes 0, 1 and 5. The projector does
not reconstruct a missing frame from support geometry. Other map modes, conflicting support
names, and incomplete or cyclic container paths keep the native operation and emit
`feature.primitive-placement-unresolved`. Dimension-only definitions without placement or
attachment properties keep an absent neutral placement and emit the same loss. The native
attachment summary product is separate from the neutral primitive frame. Multiple parent
containers violate product or feature-tree ownership and refuse the document.

## Revolution admission

The neutral revolution axis retains the normalized resolved direction, origin, and native
axis link. Reversal negates the direction. One-sided, symmetric, and two-sided fixed angular
extents use the right-hand rule defined in the specification. Both two-sided angles are retained.
Revolution Type 1 uses last-intersection termination; Groove Type 1 uses through-all termination.
Types 2 and 3 use first-intersection and selected-face termination.
A nonzero Type with `Midplane=true`, an unknown Type, an invalid flag, a missing required
selection, a malformed axis origin, a zero axis, or an angular extent outside one turn remains native and emits
`fcstd/feature.revolution-not-transferred`. Two fixed sides must have a sum at most one turn.
An inactive `Angle2` does not change a Type 0 extent.
