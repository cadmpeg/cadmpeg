# Format support

This document lists the repository's read, write, and round-trip support. Each codec has one ladder score per declared envelope and a profile for each domain.

SolidWorks `.sldprt` has the broadest native read/write path and serves as the [reference format](roadmap.md#reference-format-solidworks-sldprt) for full semantic support. Every native format remains incomplete.

## Support ladder

The L0–L9 ladder measures how much source semantics a codec recovers for use. L0–L4 add usable content categories. L5–L9 require complete coverage of the categories below them.

### Scoring rules

1. **Strict and cumulative.** A codec earns Ln by passing every gate through Ln. Its score equals the highest level whose gates all pass.
2. **Integer levels.** Capabilities above the score appear as extras. The ladder has no fractional or plus-marked levels.
3. **Resolve doubt downward.** A partially met gate fails.
4. **Require a usable slice.** A capability rung requires working support across mainstream files in the declared envelope. A single fixture, entity census, or opaque record capture cannot satisfy it.
5. **Pass inapplicable gates.** A format definition may establish that its document kind cannot contain a category. Missing fixtures cannot establish inapplicability.
6. **Score each envelope.** A codec declares version and layout-variant bands and receives one score per band. A single version can earn L9. State discontinuous support per band.
7. **Qualify the evidence.** Every score is **claimed** when code exists, **tested** when fixtures exercise it, or **proven** when it passes the [roadmap's](roadmap.md#progress-gates) representative-corpus, byte-accounting, round-trip, and fuzzing gates.

### Levels

- **L0: Parsed.** The codec detects the format and document kind, parses container framing, and extracts document metadata plus a preview image or tessellation.
- **L1: Opened.** The codec navigates sections, directories, streams, compression, and checksums; identifies governing versions and layout variants; enumerates embedded assets and external references; and names undecoded content.
- **L2: Geometry.** Typed IR contains placed points, analytic curves and surfaces, and NURBS with correct units, parameterization, and model-space placement. Prototypes, datums, and display meshes require placement or exact geometry to satisfy this level.
- **L3: Model.** Connected B-rep from bodies through vertices includes ownership, orientation, trimming, placement, and transforms. Structural validation passes across the band. Connected topology may contain unknown carriers.
- **L4: Design records.** Features carry operation semantics, such as an extrude's profile, direction, and extent. Sketch geometry, ordering, and dependencies expose the history. Replayability belongs to higher levels.
- **L5: Shape complete.** Every geometry carrier family and topology case in the band decodes. Mainstream band files contain typed carriers throughout, and body and face colors transfer.
- **L6: Design complete.** Complete sketch constraints, dimensions, parameters, and expressions; every feature family with full operation semantics; configurations; history coherent enough to re-derive the model.
- **L7: Product.** Components, occurrences, placements, external references, mates, and persistent identity across the structure. Part-only formats satisfy inapplicable gates.
- **L8: Full document.** Presentation, PMI, annotations, and drawings where carried; application data typed or deliberately preserved with identity; complete byte accounting that classifies every byte as typed, structural, or part of a named opaque record.
- **L9: Writes back.** Semantic native writing supports edits, source-less generation, target-version selection, explicit rejection, and unsupported-record survival or refusal. Independent native applications accept verified round trips. Byte replay and bounded patching count as extras at lower levels.

### Current scores

| Codec                                      | Score          | Extras above score                                                                                    |
| ------------------------------------------ | -------------- | ----------------------------------------------------------------------------------------------------- |
| FreeCAD `.FCStd` (schema 4, file 1)        | **L9 tested**  | deterministic retained writes, checked edits, source-less typed application graphs                    |
| Autodesk Fusion `.f3d`                     | **L4 tested**  | native replay + patch + broad source-less generation, procedural carriers, ACT/Design/history records |
| SolidWorks `.sldprt`                       | **L4 tested**  | typed features, sketches, parameters, configurations, native replay + bounded generation              |
| Rhino `.3dm` (archive 50/60/70/80)         | **L9 tested**  |                                                                                                       |
| CATIA V5 `.CATPart` (standard-nested band) | **L2 claimed** | conditionally connected B-rep                                                                         |
| Siemens NX `.prt` (selected or terminal-lineage-resolved body images) | **L4 claimed** | procedural carriers, topology attributes, external-dependency inspection, named arrangements |
| Siemens NX `.prt` (unselected multi-partition history) | **L2 claimed** | connected candidate B-reps, external-dependency inspection                              |
| CATIA V5 `.CATPart` (other layout bands)   | **L1 claimed** |                                                                                                       |
| Creo Parametric `.prt`                     | **L1 claimed** | partial placed geometry, connected topology, sketches, constraints, parameters, expressions, features |
| Rhino `.3dm` (V3/V4)                       | **L1 tested**  | metadata and bounded object-record retention                                                          |
| Rhino `.3dm` (V1/V2 and archive 5)         | **L0 tested**  | header-only inspection; decode is rejected                                                            |
| STEP Part 21 AP242 editions 1–3            | **L9 tested**  |                                                                                                       |
| STEP Part 21 AP203 editions 1–2 and AP214  | **L9 tested**  |                                                                                                       |
| IGES 5.3 Fixed ASCII mechanical/document   | **L8 tested**  | read only                                                                                             |

Each current score applies to the envelope described in its profile.

## FreeCAD `.FCStd`

**Model:** ZIP-packaged application object/property graph with exact-shape and presentation side
entries

**Primary envelope:** `SchemaVersion=4`, `FileVersion=1`, including core App, Part, PartDesign,
Sketcher, Spreadsheet, Assembly, TechDraw, GUI records, text and binary B-rep entries, and
identity-preserving extension objects. GUI state, thumbnails, persistent element maps, and
string-hasher tables are independently optional.

**Ladder: L9 tested.** The generated public-corpus profile passes every cumulative read gate for
container, persistence, geometry, connected model, design records, appearance, design completeness,
product structure, presentation, drawings, annotations, and deliberately retained application data,
plus semantic writing, edits, source-less generation, target selection, and unsupported-record
survival.
Schema versions 2 and 3
and earlier layout bands are separate legacy profiles and are identified and explicitly refused.

- **Read profile:** Complete for the primary envelope. Text and binary exact shapes, connected topology, sketches,
  constraints, core design operations, product links, TechDraw, semantic annotations, Mesh,
  Points, embedded assets, inert extension data, and exact physical/logical byte accounting are
  implemented. See the generated coverage profile for the current cumulative gate result.
- **Native write:** Complete for the declared write envelope. Schema 4/file 1 retained documents
  regenerate deterministically while preserving every unedited XML record and named side entry.
  Checked leaf property edits and side-entry replacements are supported. Recursive typed
  application graphs can be generated without a source archive. Unsupported schema/file targets,
  cross-band transcoding, and edits lacking a typed nested-value serializer are explicitly refused.
- **Round trip:** Every manifested public fixture writes deterministically, decodes to the same
  semantic fingerprint, accepts a typed property edit, and retains every named entry by identity
  and digest. FreeCAD 1.1.1 accepts all written fixtures; a representative full design document
  also recomputes, saves, and reopens with object identities and types unchanged.

See [`formats/freecad_fcstd.md`](formats/freecad_fcstd.md),
[`formats/freecad_fcstd-open-items.md`](formats/freecad_fcstd-open-items.md), and
[`formats/freecad_fcstd-coverage.md`](formats/freecad_fcstd-coverage.md).

## Status terms

- **None:** the repository lacks an implementation for the domain.
- **Inspect:** cadmpeg identifies and reports the structure without transferring it into typed IR.
- **Partial:** cadmpeg transfers a typed subset and reports or preserves the remainder.
- **Complete:** the domain satisfies the corpus-coverage, byte-accounting, validation, round-trip, and fuzzing gates in the [roadmap](roadmap.md#progress-gates).

Every current profile contains incomplete domains. Current claims rely on code, generated fixtures, and explicit loss paths; broader corpus evidence remains to be recorded.

Entity provenance and domain status measure different properties. `byte_exact`, `derived`, `inferred`, and `unknown` describe how cadmpeg obtained one IR value.

## At a glance

- **FreeCAD `.FCStd` schema 4/file 1 (L9 tested):** complete primary-envelope document recovery,
  including exact geometry, design history, product structure, presentation, drawings, annotations,
  retained application data, exact byte accounting, deterministic semantic writes, checked edits,
  and source-less typed application graphs.
- **Autodesk Fusion `.f3d` (L4 tested):** design records, partial B-rep and appearance reads, byte-exact replay, native patching, and source-less generation.
- **SolidWorks `.sldprt` (L4 tested):** connected model reads, typed design records, native writes, and round trips.
- **Rhino `.3dm` (L9 tested for archive 50/60/70/80):** complete built-in model, product, presentation, annotation, metadata, application-data retention, and byte accounting, plus bounded semantic native writing with source-less generation, supported edits, explicit target selection, and atomic refusal. V3/V4 score L1; V1/V2 and archive 5 score L0.
- **CATIA V5 `.CATPart` (L2 claimed for the standard-nested band):** exact carriers and conditionally connected topology. Other layout bands score L1. Read only.
- **Siemens NX `.prt` (L4 claimed for single-body, `RMFastLoad`-selected, and terminal-feature-lineage-resolved body images; L2 claimed for unresolved multi-partition history):** exact carriers, connected topology, ordered feature operations, expressions, and typed sketch-point dependencies. Read only.
- **Creo Parametric `.prt` (L1 claimed):** container navigation plus partial placed geometry, connected topology, sketches, constraints, definition-scoped dimension parameters, expressions, and typed feature operations. Decoded dimension rows transfer independently of incomplete table tails; table completeness gates only ordinal relation joins. Curve-equation expressions retain case-insensitive bindings and complete scoped dependency symbols, resolve unambiguous decoded dimension values in relation units, bind unique dimension dependencies, evaluate admitted numeric and string values, and retain prohibited datum-curve constructs without deriving values. The cumulative L2–L6 gates remain incomplete. Read only. See the [coverage contract](formats/creo_prt-coverage.md).
- **STEP Part 21 (L9 tested):** AP242 editions 1–3 transfer exact geometry, connected topology, products, tessellation, presentation, PMI, and named opaque application records with complete byte accounting. AP203/AP214 transfer their geometry, topology, product, and presentation domains. Semantic writing supports all six target schemas, source-less documents, typed edits, strict atomic refusal, and independently checked round trips.
- **IGES 5.3 Fixed ASCII mechanical/document profile (L8 tested):** complete read-only fixed-card framing, geometry, topology, product records, presentation records, and byte accounting for the declared envelope.

## IGES

**Model:** IGES 5.3 entity graph

**Ladder: L8 tested for the IGES 5.3 Fixed ASCII mechanical/document envelope.** Compressed ASCII, Binary, pre-5.3 Fixed ASCII, and extensions are separate envelopes and do not inherit this score.

### Envelopes

- **IGES 5.3 Fixed ASCII mechanical/document.** The 80-column representation containing Start, Global, Directory Entry, Parameter Data, and Terminate sections; the geometry, topology, product, presentation, annotation, drawing, associativity, and property entity branches listed by `corpus/iges-envelope-a.toml`; and no extension entity outside that matrix. The codec is read only.
- **Pre-5.3 Fixed ASCII.** Version-specific legacy envelope. Detection and exact version reporting do not imply semantic compatibility.
- **Compressed ASCII.** Distinct representation envelope. Fixed ASCII support does not apply.
- **Binary.** Distinct representation envelope. Fixed ASCII support does not apply.
- **Extensions.** Named extension envelopes only. An unregistered entity type or form remains inspectable and prevents a Fixed ASCII mechanical/document score above the last cumulative gate that does not require its semantics.
- **Finite-element analysis.** Types 134, 136, 138, 146, 148, and 418 form an adjacent analysis envelope and are excluded from the mechanical/document profile.
- **Electrical, artwork, and schematic.** Type 125 and Type 402 Forms 8, 10, and 11 form an adjacent electrical-presentation envelope and are excluded. Types 132, 320, and 420 remain in the mechanical/document profile only for typed network definition, connection identity, and occurrence structure.
- **Macro and extension definitions.** Type 306 belongs to extension-envelope declaration and is excluded from the closed mechanical/document profile.

### Ladder applicability

- **L0 preview/tessellation is inapplicable.** The envelope has no thumbnail or display-mesh record. Detection, fixed-card framing, document kind, and Global metadata satisfy the applicable L0 semantics. Derived tessellation is an optional recovery product and is not required for L0.
- **L3 connected-model semantics split by source topology.** Explicit manifold B-rep records must produce a connected body-to-vertex graph with source sharing and orientation. Trimmed and bounded surfaces carry face-local boundary identity but no cross-face shared-edge identity; they must produce valid sheet regions without invented adjacency. This is connected recovery of the topology represented by each source object.
- **L4 is inapplicable.** The envelope contains geometry, topology, presentation, associativity, and application records but no ordered feature-operation history or replayable sketch history.
- **L6 is inapplicable.** The envelope contains dimensions and associativity as document semantics, not a complete parametric design system with constraints, expressions, configurations, and re-derivable feature history.
- **L7 mates are inapplicable.** Product definitions, occurrences, groups, placements, external references, and persistent source identities are required. The envelope has no assembly-mate constraint model.

### Read profile

- **Container and versions: Complete.** Bounded detection, inspection, and decode cover IGES 5.3 Fixed ASCII cards, section order and counts, Global delimiters and metadata, Directory pairs, Parameter records, reference findings, entity/form census, physical line endings, and post-Terminate bytes. Compressed ASCII and Binary are detected and refused by name. Pre-5.3 Fixed ASCII reports its version and is refused for semantic decode.
- **Geometry: Complete.** Admitted point, vector, analytic curve and surface, conic, composite, copious-data, parametric-spline, rational B-spline, ruled, revolved, tabulated, offset, bounded, trimmed, face-local boundary, CSG primitive, sweep, and Boolean carriers decode into exact neutral geometry or typed native construction records. Units and nested definition, occurrence, entity, and reflected transformations are applied once.
- **Topology: Complete.** Type 141/142/143/144 face-local boundaries produce validated sheet regions without inferred adjacency. Type 186/502/504/508/510/514 records produce validated shared vertex, edge, coedge, loop, face, shell, region, and body graphs, including seams, voids, open shells, and explicit non-manifold radial rings. Invalid candidates commit no partial topology.
- **Design intent: Inapplicable.** L4 and L6 semantics are absent from the declared format model.
- **Product structure: Complete.** Typed native records preserve subfigure and network definitions, occurrences, array occurrences, solid assemblies and instances, groups, connect points, external references without implicit opening, attributes, units, associativities, persistent Directory identity, and separate placements.
- **Presentation and metadata: Complete.** Global metadata, standard and definition colors, line fonts, text fonts and templates, views, visibility, drawings, notes, leaders, dimensions, symbols, witness geometry, sectioned areas, drawing properties, and admitted Type 406 property forms retain typed identity and links. Neutral appearance transfers where the common IR defines it; drawing and PMI semantics remain native.
- **Recovery and retention: Complete.** `native.iges` retains physical cards, generic entity records, typed domain arenas, raw token values and spans, links, and source identities. `SourceFidelity` retains exact opaque byte records with source range, length, bytes, and SHA-256. Its byte ledger classifies Global values and delimiters, Directory fields and reserved bytes, Parameter tokens, delimiters, comments, back-pointers, card framing, line endings, Terminate counts, and post-Terminate bytes with exact nonoverlapping source coverage.

### Write and round trip

- **Native write: None.** Writing is outside the envelope.
- **Round trip: None.** Writing is outside the envelope.

## Rhino `.3dm`

**Model:** 3DM object graph

**Ladder: L9 tested for archive 50/60/70/80; L1 tested for V3/V4; L0 tested for V1/V2 and archive 5.**

### Read profile

- **Container and versions: Partial.** Archive 50/60/70/80 and V3/V4 use bounded chunk, checksum, table, object-record, class, attribute, userdata, properties, settings, layer, and EOF framing. V1/V2 and archive 5 expose the 32-byte header and archive version only; normal decode returns `NotImplemented`.
- **Geometry: Complete for archive 50/60/70/80 built-in shape carriers.** Points, point clouds, lines, arcs, polylines, polycurves, curves on surfaces with parameter/support carriers, persistent polyedge references, NURBS curves, NURBS surfaces, NURBS cages and morph controls, plane surfaces, clipping-plane carriers, revolution surfaces, sum surfaces, hatches with placed boundary loops, detail-view boundaries, meshes, extrusions, and SubD control cages transfer into typed IR. Registered legacy identities for polycurve, NURBS curve, NURBS surface, revolution surface, and Brep use the same typed readers as their current identities. Runtime proxy and component-reference classes do not carry independent archive payloads. Lengths and length-valued tolerances convert to millimeters; angles, unit vectors, knot values, UV values, relative tolerances, and hatch pattern scale remain unscaled. Third-party classes and future payload versions remain native unknown records with bounded bytes or a complete-record length and digest. V3/V4 geometry remains unknown.
- **Topology: Complete for archive 50/60/70/80 built-in Brep cases.** Breps transfer atomically into connected body, region, shell, face, loop, coedge, edge, vertex, 3D-curve, surface, and pcurve graphs. Invalid Brep topology retains the source record and decoded child carriers without committing a partial graph.
- **Tessellation: Partial for archive 50/60/70/80.** Mesh vertices, normals, faces, texture coordinates, colors, surface parameters, and ngons transfer where their channel invariants pass. Unsupported cache and channel metadata remains retained.
- **Design intent: Complete for the built-in 3DM design model.** Native revolution, sum-surface, polycurve, persistent polyedge-reference, curve-on-surface, extrusion, hatch, NURBS-cage, and curve/surface/cage morph constructions transfer with exact solved carriers or complete persistent-reference semantics. Morph operations retain captive identities, localizers, tolerances, preview mode, and structure-preservation mode. Built-in history records transfer as ordered native operations with persistent record identity, command identity, command version, every constructible built-in value family, complete object selections, polyedge constructions, SubD edge chains, object antecedents and descendants, and unambiguous producer dependencies. Numeric parameter identifiers and command UUIDs remain native identities because 3DM carries no application-independent names, expressions, constraint taxonomy, or feature-family schema for them. Modern and legacy V5 dimensions, text, leaders, center marks, text dots, hatches, and detail views transfer with their complete built-in definition and display state. Embedded history geometry transfers as unit-normalized geometry, complete topology, and exact construction semantics.
- **Product structure: Complete.** Definitions retain persistent identity, archive index, name, description, URL, units, ordered members, definition kind, nested link depth, appearance policy, and structured external-file identity. Occurrences retain persistent identity, definition identity, parent memberships, composed placement, visibility, name, and exact source-object association. Static and linked-and-embedded members expand recursively. Linked definitions without local geometry remain structured external product definitions. The 3DM product model does not carry assembly-mate semantics.
- **Presentation and metadata: Complete.** Typed records cover layers and hierarchy; complete object display attributes and rendering-material bindings; materials and texture slots; texture mappings; embedded and Windows bitmaps; groups; lights; linetypes; hatch patterns; text styles and font characteristics; dimension styles; global annotation, grid, and render settings; saved and active views; cameras, frusta, clipping planes, page settings, construction planes, wallpaper, and trace images; previews; notes; revisions; application identity; units; selectors; and file URLs. Modern and V5 annotations retain text, formula, placement, leader geometry, style identity or index, alignment, wrapping, scaling, and text-dot display state. Third-party classes, class userdata, attribute userdata, render userdata, and future extensions remain named exact records with class/item or record identity.
- **Recovery, retention, and accounting:** Chunk boundaries isolate semantic failures. The native byte ledger partitions the complete source without gaps or overlaps into typed archive-header bytes, structural framing/checksum/end-marker bytes, and named opaque complete-record bytes. Every non-object record has exact retained bytes; every object record links to an exact retained unknown-record entry. Complete unknown records are retained within per-record and per-document limits; larger records retain exact length and SHA-256 digest. Invalid Brep, extrusion, SubD, and instance candidates do not commit partial topology.

### Write and round trip

- **Native write: Bounded.** Explicit archive 50, 60, 70, and 80 targets write source-less point objects, grouped point clouds as free-vertex bodies, circles, native-canonical rational and non-rational NURBS curves, planes, native-canonical rational and non-rational NURBS surfaces, connected polygon sheet Breps with multiple faces, shared manifold edges, disjoint outer and inner loops, line or full-domain nonperiodic rational and non-rational NURBS edges, closed manifold planar solid Breps, bounded nonperiodic rational and non-rational NURBS faces, and standalone triangle meshes. Generated archives contain the canonical table sequence, a persistent default layer, and object attributes bound to that layer. Multiple independently owned Breps, free-vertex groups, and standalone geometry coexist in one object table; each Brep is preflighted and serialized from its isolated ownership graph. Every generated object receives a deterministic native UUID derived from its IR identity. Every NURBS-face coedge carries an explicit line or full-domain nonperiodic NURBS pcurve. The pcurve remains inside the surface parameter domain and maps through the surface onto the directed C3 edge within the declared face, edge, and pcurve tolerances. Planar NURBS edges retain their C3 carrier and generate an exact projected NURBS C2 for each directed face use. Explicit line and full-domain nonperiodic NURBS pcurves are written when they exactly equal that directed projection; their parameter range and fit tolerance are retained. Pcurves with incompatible geometry, ownership, wrapper state, native tail state, parameter domain, or surface/edge agreement are rejected. Writable Breps retain edge parameter domains, directed coedge rings, radial edge incidence, sheet or solid body classification, and vertex and edge tolerances. Solid output requires two opposite directed uses of every edge, a connected shell, and nondegenerate face loops. Free-vertex bodies and writable Breps retain object names, RGBA colors, and visibility through deterministic object attributes. Mesh normals, UV values, colors, surface parameters, and curvature channels retain their native scalar encodings. Archive 60, 70, and 80 use the double-vertex mesh extension; archive 50 uses float vertices and reports quantization. Unsupported arenas, cross-object topology carriers, periodic surface topology, non-manifold topology, other display state, native records, shared free-vertex points, strip topology, unknown mesh channels, and noncanonical NURBS contracts are rejected before output.
- **Round trip: Bounded.** Generated archives decode through the native reader for every supported target and are accepted by an independent native application for archive 50, 60, 70, and 80. Exact geometry and grouping are retained for the writable families except explicitly reported archive-50 mesh quantization. A decoded archive produced by this writer can be edited and semantically regenerated when its native namespace contains only the writer's accounting records, default layer, default object presentation, and supported object records. Any additional retained Rhino arena or nondefault native presentation state is refused rather than silently dropped or replayed against edited semantics.

See [`formats/rhino_3dm.md`](formats/rhino_3dm.md) and [`formats/rhino_3dm-open-items.md`](formats/rhino_3dm-open-items.md).

## SolidWorks `.sldprt`

**Kernel:** Parasolid

**Role:** reference format for full semantic support

**Ladder: L4 tested.** Unknown geometry carriers and topology cases block L5. Incomplete sketch constraints and feature families block L6.

### Read profile

- **Container and versions: Partial.** The codec validates CRC-framed blocks, enumerates cache cells and the tail directory, extracts active Parasolid partitions, and preserves the source image. Coverage across historical schemas remains incomplete.
- **Geometry: Partial.** Analytic and NURBS surfaces and curves transfer into typed carriers. Offset, swept, blend, intersection, and other unsupported families remain opaque or produce unknown carriers.
- **Topology: Partial.** The codec builds body, region, shell, face, loop, coedge, edge, and vertex ownership for supported layouts, including multiple regions and shells per body. Schema-32001 and schema-33103 solid and sheet regions follow their native region/lump/shell chains. Schema-36001 single-region solids follow their complete bidirectional root lattices. Schema-33103 interleaved faces partition by native adjacency components rather than stream intervals. Disc14 faces partition through native region, shell, face-use, and face geometry rings. Partition face membership excludes superseded deltas geometry; deltas update referenced points and complete missing subordinate records. Periodic cylinder seams follow the stored two-loop convention, and face orientation follows bridge-anchored coedge parity. Several pcurves are derived. Older body layouts remain open.
- **Tessellation: Partial.** Display-list geometry transfers into tessellation arenas and can be regenerated. Stable face-to-triangle ownership remains open.
- **Design intent: Partial.** Configurations transfer as neutral records with material and property overrides and retain their configuration-specific solids. Active configuration names resolve uniquely and take precedence over the active geometry partition; unresolved active identity is reported. Geometry partitions without native configuration definitions produce explicitly reported inferred states. Keywords history retains feature element tags, exact containment including id-less nodes, order, names, suppression, dimensions, expressions, and attributes. Arithmetic parameter expressions evaluate across unambiguous dependency references with dimensional type checking. Semantic PMI dimensions enrich uniquely owner-qualified parameters; unbound records and native dimension subtypes are reported. Explicit feature output scopes resolve to model bodies; unresolved non-empty scopes are reported. Unknown operation families retain their kind, dimensions, and non-parameter attributes in the neutral native-operation definition. Reference planes, axes, points, and coordinate systems retain complete model-space placement. Planar profile B-reps nested in feature-input lanes transfer as placed sketches with solved lines, circles, arcs, ellipses, and rational or non-rational NURBS, plus oriented profile loops. Boss and cut extrusions retain blind, symmetric, two-sided, through-all, and native-face termination, explicit direction, draft, profile, and Boolean operation. Explicit-axis revolutions retain one-sided, symmetric, and two-sided angular extents, profile, axis placement, and Boolean operation. Profile sweeps, lofts including boundary boss and cut forms, ribs, bending, twisting, tapering, and stretching flexes, drafts with selected faces and neutral planes, direct body Boolean combines, body deletion and isolation, body scaling about an explicit center, face deletion and replacement, face offset/translation/rotation, spherical and elliptical domes, linear and circular patterns, mirrors, constant and variable-radius fillets with selected edges, dimensional chamfers with selected edges, shells with removed faces, face thickening in either direction or both directions, and simple, counterbore, and countersink holes with explicit face, position, direction, and blind or through-all termination project to neutral operations and write edits through retained native records. Decode reports parameters without evaluated scalars, expressions with unresolved quoted parameter references, history records with ambiguous identities or unresolved structural references, native sketch relation records omitted before constraint projection, native-only sketch geometry and constraints, native-only feature definitions, every typed feature retaining unresolved required operation semantics, and body delete/keep features whose retention mode is unresolved. Sketch constraints and other operation families remain open.
- **Product structure: None.** `.sldprt` support covers parts only.
- **Presentation and metadata: Partial.** Base colors, appearance bindings, previews, SolidWorks XML metadata, units, and selected attributes transfer. Full appearance precedence and all embedded metadata stores remain open.

### Write and round trip

- **Native write: Partial.** Unchanged IR with a retained source image writes byte for byte. Modified or source-less supported IR regenerates native blocks and a section directory.
- **Semantic write limits:** at most five regions per body and six shells per solid region; sheet regions require one shell. Ellipses whose declared major radius is smaller than the minor radius, elliptical or non-acute cones, signed sphere or torus parameterizations, explicit face names, stored edge parameter ranges, periodic NURBS carriers, NURBS surface degrees outside 1–8 or shapes that do not re-infer identically, and unbounded appearance data are not encoded.
- **Round trip: Partial.** Byte-exact unchanged-file and semantic regeneration paths have generated-fixture tests. The public version and feature matrix remains to be built.

See [`formats/sldprt.md`](formats/sldprt.md) and [`formats/sldprt-open-items.md`](formats/sldprt-open-items.md).

## Fusion 360 `.f3d`

**Kernel:** ASM, derived from ACIS

**Ladder: L4 tested.** Undefined carrier payloads and tolerant-topology variants block L5. Native writing exceeds the scored gates.

### Read profile

- **Container and versions: Partial.** The codec joins the top-level manifest's asset-folder UUID to the matching per-asset manifest, scopes B-rep selection to that design folder, and composes every `.smb` and `.smbh` geometry blob referenced by the Design body map. Non-null body-map keys select matching nonnegative ASM body keys and exclude null-key bodies; when every body key is null, keys select zero-based body-record ordinals. Selected body-connected components retain blob-qualified identities before merging. History and header streams are selected independently from geometry blobs. Linked Protein, Design, MetaStream, and ACT records also decode. In-memory decoding accepts archives through 1 GiB, individual inflated entries through 512 MiB, and at most 1 GiB total declared inflated content. Entry declarations above these limits are rejected before payload allocation.
- **Geometry: Partial.** Analytic surfaces and curves, cached NURBS carriers, construction-backed cache-less translational-extrusion and surface-helix carriers, cache-less exact helix-curve carriers, parameterizations, signed radii, point-degenerate curves, exact, compound, ruled, sum, revolution, offset, rolling-ball, pipe, taper-family, loft, and G2-blend spline surfaces, exact-cache curve, compound/deformable curves, helix, vector-offset, surface-offset, spring, subset, projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, context-first blend/surface/parametric/skin constructions, and cache-first blend constructions with null, analytic, referenced, or inline NURBS surface/UV supports transfer under modern and legacy subtype names. Law-dependent net/skin/sweep forms, variable/vertex blends, and related families remain incomplete when no solved cache resolves.
- **Topology: Partial.** Shell-reachable solid, sheet, wire, and mixed-dimensional general bodies transfer with shells, faces, loops, coedges, edge-ring wires, point wires, edges, and vertices. Tolerant coedges transfer their local 3D NURBS use curves and loop-ordered parameter intervals independently of the shared edge carrier. Closed edge incidence classifies solid bodies, open face incidence classifies sheet bodies, face-less wire membership classifies wire bodies, and bodies containing both faces and wire edges classify as general. Unsupported surface records retain topology with unknown geometry; some procedural edges and explicit pcurves remain unresolved.
- **Tessellation: None.** Fusion display meshes remain outside the IR tessellation arena.
- **Design intent: Partial.** Document user parameters, sketch dimensions, and construction inputs transfer with source expressions, canonical evaluated values, dependency identities, local order, and full-fidelity native references. Coincident, midpoint, reflection-symmetry, curvature-continuity, orientation-preserving multi-pair offset, parameter-backed two-locus distance, parameter-backed angle-to-sketch-axis, single-locus radius and diameter, and parameter-driven rectangular-pattern relations transfer with typed operands. Rectangular patterns retain two unit directions, adjacent-instance spacing and count parameters, count including the seed, and complete instances whose fixed entity order and grid indices are proved by exact solved translations. Typed planar sketch curves form ordered closed profile loops when endpoint incidence is unambiguous, including bounded faces in branched line graphs. Nonplanar sketch lines, circles, arcs, and nonperiodic NURBS transfer into separate model-space spatial-sketch arenas with their owning Design placement applied. Extrude profile references distinguish an entire sketch, exact solved loop indices, exact bounded regions with immediate hole loops, and an unresolved native selection within a known sketch. Paired, repeated counted-, and other null-locus dimension frames retain their ordered sketch operands and role codes. Recipe-backed sketch dimensions transfer as native constraints with their complete ordered recipe-record operands. Every dimension without a typed locus or recipe frame transfers as a parameter-backed native constraint retaining its empty or payload-bearing companion. Indexed parameter scopes retain their family-local ordinal, current and preceding ASM delta-state identities, ordered record-reference tables, state-linked cross-family feature dependencies, and topologically valid neutral construction order. They transfer as neutral Sketch nodes and typed blind, two-sided, and to-face Extrude nodes with profile-history dependencies, signed normal direction, independent first- and opposite-side draft angles, profile-plane, signed offset-profile-plane, or selected-face starts, signed termination-face offsets, and join, cut, intersect, or new-body result operation. Native scopes also retain their one-sided distance, one-sided to-face, and two-sided distance discriminators, typed body/profile/face operand groups with ordered start and termination face roles, face operands resolved to exact face-recipe envelopes, counted i32 recipe nodes, and active B-rep candidate sets; neutral selected-face operands are disambiguated by membership in the scope's preceding history state. Ordered counted construction-operand groups, nested operand-identity chains and fixed persistent identities, ordered counted selection members resolved against persistent sketch geometry when possible, and typed Fillet and Chamfer nodes with dimensional forms and counted groups of ordered edge-recipe operands and complete i32 recipe programs also transfer. Fillet nodes retain each ordered edge group as a separate neutral radius assignment with its tangency weight. ASM history states, snapshot revision-reference runs, snapshot record revision identities, complete guarded per-state entity-version maps, fully normalized per-state RecordTables, re-derived stable topology and carrier graphs, and typed inserted/deleted/updated state transitions transfer. Unambiguous scope transitions trace changed topology and carrier slots through historical ownership to active body outputs. Design assignments, sketch-side records, construction recipes, body persistent-ID histories, variable-width face/edge design-reference tags, persistent references, MetaStream identities, and ACT channels transfer. Decode reports count native feature definitions, omitted feature scopes, retained native sketch constraints, omitted sketch relations, omitted dimensions, profile selections, face selections, edge-recipe selections, and lost edge selections independently for active and suppressed history. Unresolved dimension-companion operation semantics, recipe-to-B-rep identity, and unresolved Extrude selection identities remain open, so the Fusion feature history is not yet replayable.
  `SpirePrimitive` scopes transfer as typed Coil nodes with exact driving dimensions, generated section, section placement, angular direction, taper, and result-body semantics.
  Extrusions driven by prior solid faces retain exact state-qualified profile faces when every counted selection member corroborates one owner face.
- **Product structure: Partial.** Body transforms and root-component records transfer. Multi-component structure and constraints remain open.
- **Presentation and metadata: Partial.** Linked source attributes, Protein appearance assets, material properties, body bindings, and per-body display visibility transfer. External material-library display names and some schema fields remain unresolved.

### Write and round trip

- **Native write: Partial.** An unchanged retained source archive writes byte for byte. The writer patches model points, common analytic and NURBS B-rep curves and surfaces, rational/non-rational pcurves, procedural caches, sketch geometry, constraints, history fields, design records, and supported appearance properties in their original records. Source-less generation writes multi-body B-reps containing solid, sheet, wire, and mixed-dimensional general topology with analytic or rational/non-rational NURBS carriers; exact, compound, ruled, sum, revolution, offset, rolling-ball, taper, loft, G2-blend, and translational-extrusion surface constructions; exact-cache, compound, deformable, helix, vector-offset, and subset curve constructions; inline rational/non-rational NURBS pcurves; placements; document tolerance metadata; direct body/face color attributes; body/face/edge persistent-design tags; coedge-to-sketch provenance; typed ASM history; Design object and construction streams; sketch geometry and constraints; ACT tables and component roots; and Protein appearances with body bindings.
- **Write limits:** General writing requires a retained source archive and the original entity and record layouts. Source-less generation supports multiple placed bodies, regions, and shells with plane, cylinder, cone, sphere, torus, or rational/non-rational NURBS faces; multiple loops; shared radial edges; line, circle, ellipse, point-degenerate, or rational/non-rational NURBS edge curves; inline rational/non-rational NURBS pcurves; cache-local tolerant-coedge NURBS use curves; face-less wire bodies with chained regions and shells; edge-ring and point wires; and mixed face-and-wire shells containing either wire form. Retained cache-local tolerant-coedge NURBS use curves support structure-preserving geometry edits. Edits outside the listed fields are rejected.
- **Procedural write invariant:** One curve has at most one procedural construction. Exact-cache, compound, deformable, helix with or without a solved cache, vector-offset, surface-offset, spring (including conditional null-carrier ranges), subset, ranged and early-close projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, and prefix-only blend/surface/parametric/skin definitions with paired analytic/NURBS surface and NURBS UV supports serialize with their native construction fields, child carriers, role fields, and solved caches. Scalar offset and blend-spine definitions remain rejected source-less until their external fields can be emitted without semantic loss. No typed construction is reduced silently to a cache-only curve.
- **Round trip: Partial.** Generated byte fixtures cover byte-exact replay and each writable geometry, history, Design, sketch, ACT, and appearance family.

See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).

## Siemens NX `.prt`

**Kernel:** Parasolid in an SPLMSSTR container

**Ladder: L4 claimed for single-body, `RMFastLoad`-selected, and terminal-feature-lineage-resolved body images; L2 claimed for unresolved multi-partition history.** The L4 envelope carries ordered feature and sketch records, resolved expression dependencies, solved sketch-point coordinates, and ordered sketch-to-datum dependencies. Multi-partition histories remain L2 unless every emitted partition receives one unambiguous terminal lineage status.

### Read profile

- **Container and versions: Partial.** The codec decodes the SPLMSSTR directory, extracts and classifies embedded Parasolid partition, deltas, and related streams, resolves short and extended segment wrappers from their encoded extension lengths, retains fixed-ID NX object-model entities at their external index boundaries, retains offset-only object-model column storage as distinct bounded blocks, and exposes strictly framed JPEG preview dimensions and asset identity.
- **Geometry: Partial.** Points, analytic surfaces and curves, typed B-spline surfaces and curves, supported type-133 trimmed curves, and exact OFFSET_SURF and BLEND_SURF procedural carriers transfer into IR. A procedural carrier has a required bidirectional link to its typed construction instead of an unknown geometry placeholder. Nested OFFSET_SURF carriers evaluate recursively over analytic, NURBS, or evaluable procedural supports with cycle rejection. BLEND_SURF evaluation remains open where finite domains or branch selection are unresolved.
- **Topology: Partial.** Active body-image bands emit connected body, shell, face, loop, fin, edge, and vertex graphs with ownership, orientation, trimming, and incidence. A typed edge carrier remains attached when its serialized trim limits fail endpoint validation; the unvalidated parameter range is omitted. Validated intersection support-UV charts attach as coedge pcurves with their parameter ranges and fit tolerances. Missing and sentinel-bearing support lanes are reconstructed atomically by inverse parameterization only when every point forward-evaluates within the effective chart tolerance; nested procedural blend spines add their cache-fit tolerance and the dependent curve records the resulting bound. Carriers with exactly equal parameterization share a complete lane. Finite-tolerance EDGE records with a null curve pointer transfer as procedural tolerant-intersection carriers over their two adjacent face surfaces; the edge vertices bound the relation. Non-null shell BODY and REGION references supply ownership identity when either record is omitted; shell layouts use either a FACE chain or a face anchor with FACE back-references. Solid and sheet kinds derive from edge incidence. Inline Parasolid coordinates are in part-model space and bodies have identity placement. A validated partition shell defines a current topology image; paired deltas topology records remain revision history. Supported non-topology replacements and tombstones use the last event for each exact key, except that a surviving topology reference keeps its partition point, curve, or surface carrier when no later replacement exists. `RMFastLoad` object membership selects every decisively represented active body image and removes inactive historical images. Without a decisive membership set, complete feature-history primary writers and later Boolean tool consumption select terminal bound partition images atomically. Feature-history composition remains unresolved when any emitted image lacks that complete lineage.
- **Tessellation: Partial.** Embedded JT 9 shape-LOD segments decode lossless and uniformly quantized coordinates, lossless and Deering normals, lossless and RGB/HSV-quantized colors, up to eight lossless or uniformly quantized texture-coordinate channels, and binary vertex flags. Topological dual-mesh connectivity and corner attribute indices reconstruct exactly. Logical shape-node to late-loaded payload ownership resolves through the property table. Common group-derived and instance-node paths accumulate affine transform attributes; each distinct root-to-shape path emits a tessellation with ordered instance provenance. Nonnegative-group triangles transfer in document units with transformed seam-preserving normals and codec-owned RGBA, texture, and flag channels parallel to deindexed vertices. Body association remains unresolved.
- **Design intent: Partial.** Feature-history record areas project ordered neutral features and retain exactly bounded operation records, byte-exact labels, and four object-index slots. `DATUM_PLANE` and `DATUM_CSYS` labels project as typed datum families with explicitly unresolved frames. Non-null header slots bind to uniquely addressed offset-only input blocks. Exact reuse of a resolved datum-plane or datum-coordinate-system construction block by a later operation input creates an ordered neutral feature dependency without assigning frame roles. Uniquely resolved parameter declarations appear as neutral parameter content on every consuming operation in serialized input order. Primary-body fields form ordered lineage dependencies and bind surviving segment body images as feature outputs; Boolean features also depend on the latest ordered tool-body writers. Unite, subtract, and intersect operations project as typed body-combine features with ordered native target/tool selections. `SEW` operations project as typed body-sew features; complete validated body operands supply the ordered body selection, while incomplete operands leave it unresolved. `TRIM BODY` operations project as typed body-trim features; one primary body and complete validated body operands supply the ordered target and tool selections, while incomplete operands leave both selections unresolved. The retained side remains unresolved. `TRIMMED_SH` and `EXTEND_SHEET` project as typed surface-trim and surface-extension operations with unresolved input selections and construction controls; output ownership does not identify their pre-operation operands. `EXTRUDE` operations project as typed extrusions; exactly one complete construction profile supplies its native profile identity, while absent or ambiguous profiles remain unresolved. A first-writer extrusion with one transferred solid or sheet output projects as new-body; later, wire, mixed-dimensional, multiple, and absent writer states remain Boolean-unresolved. Direction, termination, and draft remain unresolved. `OFFSET` operations project as typed surface-offset features; one output body with owned offset carriers at one exact carrier-normal distance supplies the support-surface selection. Each support resolves to a neutral face when ownership is unique and distinct; uniform face sense then supplies the face-normal-relative neutral distance with orientation-corrected sign. A single solid `THICKEN_SHEET` output body whose owned offset carriers have one exact finite nonzero signed distance projects one-sided thickness, while matched opposite equal-magnitude carrier sets over identical supports project twice that magnitude and a both-sided operation. The same support-face resolution rule applies; uniform agreement between a one-sided distance sign and resolved face senses projects the forward or reverse side. A single `BLEND` or `FACE_BLEND` output body whose owned blend carriers are circular projects with an exact common constant radius or a retained structural radius form. Complete bipartite support-surface graphs with unique face ownership project as resolved face blends. Other `BLEND` support graphs retain an unresolved edge selection as fillets; other `FACE_BLEND` support graphs retain unresolved face selections without changing family. `BLOCK` projects as a typed rectangular primitive; complete dimensions project in native order. With those dimensions, exactly one single-region, single-shell solid output body with a complete owned three-band orthogonal planar extent system supplies the derived right-handed rigid placement when exactly one dimension-to-band permutation matches. `SIMPLE HOLE` projects as a typed simple-hole operation; a `Hole_GeneralHole_Simple_Through_*` payload template projects through-all termination. A complete single construction group partitions through-hole operations by exact output-body ownership; each output is one connected solid region and shell, and equal-cardinality uniform owned bore cylinders supply one permutation-invariant diameter per body. Distinct output bodies may carry distinct diameters. The complete ungrouped operation set uses the same ownership rule. The complete `HOLE PACKAGE` operation set uses that ownership and bore-bijection rule without a simple-hole template. Every body partition with one hole operation and one bore supplies a canonical model-space axis position; common parallel axes supply direction independently of diameter correspondence. Complete output-body-owned pairs of coaxial conical boundary faces supply one common entry-and-exit chamfer diameter and included angle per body. Each neutral simple-hole feature owns its exact native template, redundantly witnessed scalar lane, and resolved construction-block references through source properties; entry-face ownership remains unresolved. `HOLE PACKAGE` retains unresolved form and entry-face fields. `RIB` and `CHAMFER` project as typed feature families with unresolved operands and dimensional fields. `Pattern Feature` and `Pattern Geometry` project as typed patterns with unresolved seed and transform fields. `SKETCH` operations project as ordered sketch history nodes with unresolved coordinate space and retain a typed sketch record joining the exact operation record to its resolved native input blocks in header-slot order. Named points bind through exact shared blocks or a unique same-store predecessor span; solved sketch planes, entities, profiles, and constraints remain unresolved. Segment tuples bind both serialized object-index aliases of each partition or plain cached-body image; either alias resolves feature selections and outputs to neutral body IDs when the image remains in the transferred model. Other named operations project as native features until their inputs are complete. Typed numeric expressions retain object identity, owning OM record, name, exact expression text, declared millimeter or degree unit, ordered exact-name parameter dependencies, and evaluated values for context-free and acyclic same-unit dependency arithmetic. Named arrangements retain ordered configuration names and default state. Exact agreement between the default arrangement and the part-level active-arrangement attribute assigns every selected current body to that active configuration; inactive configuration body states remain unresolved. Complete intermediate-body relations and other operation semantics remain open.
- **Product structure: Inspect.** Every indexed EXTREFSTREAM record retains its exact boundary and digest. Handle-set records additionally retain their declared counts, four raw ID slots, normalized persistent handles, closing-duplicate form, opaque-tail lengths, and exact byte offsets. Their four ID slots resolve atomically through the same-stream string table; slot zero names the child `.prt`, slot two supplies its directory, and each complete handle-set record binds to those child records. Equal persistent handles create explicit cross-stream identity edges between EXTREFSTREAM records and bounded OM records. End-anchored child-part names and paths transfer as ordered typed native records and also appear in source dependency metadata. Assembly graph instances, placements, and constraints remain open.
- **Presentation and metadata: Partial.** Length-bounded Parasolid attribute-class declarations retain stream-local class and field-record identities, exact printable class name, declared field count, typed primary storage, ordered catalog references, variant header words, and inflated-stream offset. Framed type-81 lists, type-82 integer lanes, type-83 finite binary64 lanes, and type-84 printable values retain exact identities, ordered reference slots, revision records, and byte spans. Every uniquely matched type-81 class discriminator resolves to its exact same-stream type-79 definition XMT independently of topology ownership. Topology records retain non-null attribute-list ownership. Unique topology-to-list and list-to-value chains transfer ordered integer, finite-double, and printable-string lanes to shell, face, loop, edge, coedge, and vertex source attributes with class-qualified family-and-ordinal names. Class-specific field assignment, colors, materials, and other presentation semantics remain unresolved.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open geometry gates include unresolved procedural-intersection branches, freeform NURBS-offset blend spines, and other unsupported record families. Open structural gates include unmatched tombstones, multi-partition feature composition, assembly records, and NX object-model field serialization.

See [`formats/siemens_nx.md`](formats/siemens_nx.md) and [`formats/siemens_nx-open-items.md`](formats/siemens_nx-open-items.md).

## CATIA V5 `.CATPart`

**Kernel:** CGM

**Ladder: L2 claimed (standard-nested band); L1 claimed (other layout bands).** L3 requires connected topology across the band. Current topology depends on resolved trim, support, and endpoint assignments.

### Read profile

- **Container and versions: Partial.** The codec decodes `V5_CFV2` containers, enumerates named outer and inner directory streams with reconstructed extents, inventories every bounded outer `FINJPL` block by stored name/type/family, reads the saved-by CATIA version/release/service-pack/hot-fix tuple, enumerates CATIA document references, and distinguishes standard-nested, FBB-only, zero-entity, float-packed, E5, and inner-without-directory layouts.
- **Geometry: Partial.** Standard-nested and FBB-only files transfer vertices, bridged planes, signed-axis analytic carriers, supported edge curves, consolidated NURBS carriers, uniquely domain-bound constant-offset constructions, and fit-free rolling-ball surface jets. E5 one-pcurve boundary supports lift analytic isoparametric lines and circles into exact 3D edge carriers and retain general nonplanar pcurves as exact parametric surface-curve constructions. Zero-entity graphs transfer analytic, NURBS, and typed procedural curve carriers. Freeform face aliases and unbridged planes remain unknown.
- **Topology: Partial.** Standard-nested and FBB-only files emit a connected body, shell, face, loop, coedge, edge, and vertex graph when trim, support, and endpoint assignment resolve. Reference-closed E5 and zero-entity graphs emit the same connected common-IR topology. Incomplete endpoint quotients and owner graphs decline atomically to disconnected geometry.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: Partial.** The summary-information JPEG preview transfers byte-exactly with its dimensions. Persistent tags, attributes, materials, and appearance bindings remain outside the IR.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open gates include endpoint incidence for additional variants, orientation signs, pcurve attachment, spline edge curves, remaining persistent edge/cache bindings, attributes, and the consolidated-stream tag resolver.

See [`formats/catia.md`](formats/catia.md) and [`formats/catia-open-items.md`](formats/catia-open-items.md).

## Creo Parametric `.prt`

**Kernel:** Granite, serialized through PSB

**Ladder: L1 claimed.** Incomplete model-space coverage across analytic and spline carrier families blocks L2. Exact plane components, selected cylinders, placed sketches, and native design records exceed the L1 gates.

### Read profile

- **Container and versions: Partial.** The codec detects `#UGC:2`, enumerates sections, identifies ND and DEPDB layouts, decodes supported PSB compact integers and floats, expands Unix-compress payloads, and retains complete counted `double_xar` scalar dictionaries with literal values and unresolved reference slots.
- **Geometry: Partial.** ActDatums and VisibGeom plane carriers transfer in model space. Finite `MdlRefInfo` lines and circular records transfer as model-space carriers; named conic records retain their endpoints, coefficients, parameters, and complete local-system slots without premature conic-family classification. Visible and nonvisible surface and curve namespaces remain separate and retain stable native identities, raw type bytes, feature ownership, topology links, exact parameter bodies, named prototype fields, and source offsets. Named surface prototypes, bounded curve parameter records, and tabulated-cylinder cubic curve replays retain typed named parameter wrappers, exact parameter and packed control-point bodies, and decoded two-coordinate control points where complete. Complete named ND plane and torus-family prototypes transfer their adjacent first positional instances from local frames and family parameters. Complementary five-coordinate hemisphere envelopes sharing a zero-major-radius prototype transfer as placed spheres. Other tagged positional torus/sphere radius overrides and complete terminal outline extents retain typed row-local fields until the same positional body establishes a complete model-space placement. Cylinder and cone prototype frames remain templates until a positional construction or feature placement establishes model space. Unbound straight `geom_type = 2c` rows transfer as planes and extrusion constructions. Replay-bound rows with a unique directrix-to-frame span assignment transfer as cubic NURBS curves, tensor-product extrusion surfaces, and extrusion constructions; other frame variants remain native. Topology-bound cylinders transfer when cap records establish their complete placement. Complete saved-section lines, arcs, and circles generate placed plane or cylinder carriers when the order and generated-entity tables bind them to a same-family surface owned by the linear sweep. Resolved linear sweeps also evaluate closed-profile vertex line carriers independently of extent trimming. A type-20127 zero-offset placement instruction resolves the blind class-917 circular section against its standard datum; the generated cap envelope aligns its saved circle with the model-space cylinder carrier. A class-913 slot fillet with two independent equal-gap parallel support pairs transfers its unique tangent cylinder. In a four-entry round table, a rowless third face inherits the complete cylinder equation of the following materialized cylinder under the schema-913 sibling invariant. A uniquely owned DEPDB section with one complete local-system frame uses its stored local z axis and origin as the section plane; this places its sketch against the stored perpendicular reference plane. Resolved rotational sweeps evaluate unbounded plane, cylinder, cone, sphere, torus, tensor-product NURBS, and non-axis profile-vertex circular orbit carriers independently of unresolved angular trimming. Explicit source-entity identifiers bind generated carriers to native surface rows. Feature-generated carriers are evaluated before native intersection curves and topology. Plane/plane intersections transfer as lines; unique plane/cylinder intersections transfer as circles, ellipses, or tangent lines; a two-generator plane/cylinder secant transfers when solved native endpoints select exactly one generator; internally or externally tangent parallel cylinders transfer their single common generator. Placed circular cones contribute perpendicular-plane section circles, tangent-plane generator lines, endpoint-selected two-generator apex sections, endpoint-selected coaxial-cone circles, endpoint-selected coaxial-cylinder circles, endpoint-selected coaxial-torus circles, and endpoint-selected coaxial-sphere tangent or secant circles. Placed spheres participate in plane-intersection and sphere-intersection circles; an equal-radius cylinder whose axis contains the sphere center contributes their single equatorial circle. Axis-normal planes contribute endpoint-selected tangent or secant torus circles. Coaxial cylinders, axis-centered spheres, and coaxial tori contribute endpoint-selected tangent or secant torus circles. Other secant cases with multiple components remain unresolved. Other analytic and spline families remain incomplete.
  Invariant-complete positional conic records additionally transfer as model-space ellipses.
  Strict-secant parallel cylinders transfer one common generator when solved
  native endpoints select exactly one of the two candidates.
  Analytic carrier pairs with one derived curve component transfer without
  solved endpoints; present endpoints must agree with that component. Multiple
  components require endpoints that select exactly one candidate.
  Positive-ratio elliptical cones transfer tangent apex generators and
  endpoint-selected two-generator apex sections.
  Coaxial positive-ratio cones with proportional transverse quadratic forms
  transfer unique or endpoint-selected axis-normal elliptical sections,
  including reciprocal-ratio sections with exchanged principal frames.
  A cylinder through a sphere center transfers its equatorial tangent circle
  or one endpoint-selected secant circle.
  Third-plane intersections resolve vertices across every transferred
  multi-component analytic circle family when exactly one point remains.
  Planes containing a torus axis contribute the two exact meridian circles;
  paired solved endpoints select one component.
  Solved native edges on derived intersection lines use start-anchored unit
  carriers and `[0, length]` parameter intervals. Exact native line carriers
  retain their source parameterization and use projected endpoint intervals.
  Complete positional and uniquely face-bound labeled UV endpoint pairs
  transfer as straight pcurves when their face-surface images uniquely agree
  with the solved coedge traversal. Their mapped midpoints select minor, major,
  or full-turn parameter intervals on circular and elliptical native edges;
  adjacent face paths must agree.
  A periodic conic used only by one-edge closed native loops uses one full
  period from its seam vertex when no native pcurve candidate is present.
  Parabolic edges recover endpoint parameters from their focal frame.
  Hyperbolic edges recover endpoint parameters after paired vertices select
  exactly one of the two analytic branches.
  Degree-one nonperiodic NURBS edges algebraically invert positive-weight
  rational line spans and transfer any bounded interval whose solved vertices
  each have one global parameter. Higher-degree carriers use their full
  intrinsic knot domain when solved vertices uniquely match its endpoints.
  A positive-weight periodic NURBS used only by one-edge closed native loops
  uses its intrinsic domain when both bounds evaluate to the seam and no
  pcurve candidate exists.
  Exact line, conic, and NURBS edges on solved planar faces project into exact
  plane-chart pcurves when no native pcurve candidate is present, preserving
  the 3D carrier parameterization and edge interval.
  Coaxial constant-parameter circular edges on cylinders, spheres, and tori,
  plus circular or elliptical edges on matching cone parallels, project into
  exact affine surface-chart pcurves under the same absence rule, preserving
  their native angular parameter and edge interval across either cone nappe
  and signed torus ring branches.
  Exact sphere and torus meridian circles project to constant-azimuth affine
  pcurves, preserving their native angular parameter and edge interval through
  sphere poles.
  Exact generator lines on cylinders and positive-ratio cones project to
  constant-azimuth affine pcurves, preserving arbitrary nonzero native line
  scales and edge intervals.
  Positive-ratio elliptical cones participate in exact point containment,
  axis-normal and oblique ellipse, parabola, and hyperbola sections, and
  plane/plane/cone vertex solving; rotational-symmetry reductions remain
  restricted to circular cones.
- **Topology: Partial.** Native half-edges and closed loops decode. Canonical curve adjacency rows retain their feature owner, orientation bytes, incident faces, next-edge links, and source offsets as native records. The first resolved linear section sweep evaluates into a connected body, region, shell, correctly oriented cap and side faces, loops, coedges, edges, and vertices with exact plane and cylinder pcurves. It supports one line/arc outer profile with pairwise-disjoint, unnested, oppositely oriented line/arc holes; analytic line/line, line/arc, and arc/arc predicates reject touching or intersecting boundaries. A one-circle profile evaluates into two planar caps and one cylindrical side face with closed circular edges, seam vertices, constant-offset side pcurves, and paired radial uses. A full-turn, one-profile line/arc revolution evaluates into a connected solid across planar, cylindrical, conical, spherical, ring-torus, and spindle-torus faces. Axis vertices collapse; off-axis vertices become closed circular edges with exact analytic pcurves and paired radial uses. Later sweeps remain withheld until their Boolean operation is resolved. Native components with uniquely solved plane/plane/plane, plane/plane/cylinder, plane/plane/cone, plane/plane/sphere, plane/sphere/sphere, plane/coaxial-cone/cone, plane/coaxial-cone/cylinder, plane/coaxial-cone/torus, plane/coaxial-cone/tangent-sphere, plane/equal-radius-coaxial-cylinder/sphere, plane/axis-centered-tangent-sphere/torus, plane/plane/axis-containing-torus, plane/tangent-coaxial-tori, plane/plane/tangent-torus, or plane/tangent-cylinder-pair vertex coordinates also transfer as connected topology. Planar multi-loop faces require one strict containment outer boundary and transfer that loop as outer with every contained loop marked inner. Single-loop native faces transfer their sole loop as outer. Native non-planar faces transfer only with one loop until their byte-backed multi-loop discriminator is decoded. Multiple analytic intersection roots remain unresolved, and unsolved carriers stay linked opaque geometry rather than being guessed.
- **Tessellation: Partial.** Complete named `prim_tristripsetwithatt` position
  arrays transfer as triangle strips with alternating winding. Primitive arrays
  without a complete position lane or persistent geometry binding remain
  native records.
- **Design intent: Partial.** Ordered stored feature-operation states and their current-state projection, the configuration driver-table root pointer, dependencies, the implicit `AllFeatur` entity/reference graph and mixed generated-entity tables, order-validated visible-to-nonvisible surface replay associations, placed and unplaced section sketches and their ordered planar-sketch history nodes, source-offset-scoped repeated sketch snapshots, typed and opaque `segtab` entities including type-10 circles, ordered saved lines, arcs, circles, and splines, typed horizontal, vertical, coincidence, point-on-object, tangency, perpendicular, parallel, equal, axial and central symmetry, same-coordinate, and radius/diameter constraints, snapshot-owned dimensions including geometry-free dimension tables and radius/diameter display semantics, curve-equation programs with scalar operators and standard mathematical functions, and cylindrical native-axis helix semantics transfer as typed or native design records. A resolved base linear section sweep carries its resolved sketch profile, direction, blind, symmetric, or two-sided extent, new-body operation, solid construction state, and evaluated output body. A resolved circular section sweep carries its resolved sketch profile, direction, blind extent, Boolean effect, solid construction state, and evaluated output body. A uniquely placed DEPDB rotational section carries its profile, axis, Boolean effect, solid construction state, native definition reference, full-turn angular extent when its angle choice is present, and evaluated output body. Repeated identical full-turn sequences remain separate native regeneration-state records.
  Solver-only section entity identifiers transfer as shared native construction
  entities, preserving complete incidence references without assigning an
  unsupported geometry family.
  Current feature display names preserve the stored `id` or `ID` form while excluding the separate one-byte state prefix.
  Named datum-plane, draft, fill, surface-merge, boundary-surface, protrusion, extrusion, subtractive cut, and revolution families retain their exact operation family when their construction inputs remain unresolved. A current recipe supplies the linear or rotational construction and Boolean effect independently of the display-name family. Row-only class-927 and class-946 features remain typed drafts and surface merges when their display names and operands are absent. Non-rotational class-916 and class-917 section sweeps remain typed subtractive and additive extrusions when placement or extent operands are incomplete. Sketch entities reference model-space curve carriers only when a unique section placement materializes that carrier; unplaced entities and isolated sketch points retain no fabricated curve reference.
  Boolean classification treats only actual body outputs and prior new-body
  sweeps as established material; consuming operations do not fabricate a
  body when their source history is partial.
  The final stored state for each feature supplies its active recipe, Boolean
  effect, schema parent, and source tag while every preceding state remains an
  ordered native regeneration record.
- **Product structure: Partial.** A unique native model-name header defines one
  part product and one root identity occurrence. The product owns every
  transferred body. Assembly component definitions, child occurrences,
  placements, and constraints remain open.
- **Presentation and metadata: Partial.** Container attributes transfer as source metadata; decode-coverage counts transfer as the decode report's coverage census. Materials and display data remain open.

`geometry_transferred` is true when any complete model-space carrier transfers.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

The principal geometry gates are per-instance analytic parameter bindings, feature-generated carrier evaluation, dense curve and spline bodies, and complete face-instance placement.

See [`formats/creo_prt.md`](formats/creo_prt.md) and [`formats/creo_prt-open-items.md`](formats/creo_prt-open-items.md).

## STEP Part 21

**Model:** ISO 10303-21 clear-text exchange with AP203, AP214, or AP242 application data

**Ladder: L9 tested for AP242 editions 1–3 and AP203 editions 1–2/AP214.** Part 28 XML, Part 26 binary/HDF5, AP242 BO-Model sidecars, and ZIP packaging are outside the declared bands. AP203/AP214 gates for constructs their schemas cannot carry are inapplicable. Part 21 exchange documents do not carry originating feature replay histories, sketch-constraint systems, or assembly mates, so L4, L6, and the L7 mate gate are inapplicable.

### Read profile

- **Container and versions: Band-wide.** The codec detects clear-text exchanges, parses headers and all DATA sections, parses edition-3 ANCHOR, REFERENCE, and SIGNATURE sections, reports schemas and external dependencies, and names every undecoded entity family.
- **Geometry: Band-wide.** Millimeter-normalized points, placements, analytic curves and surfaces, polylines, rational and non-rational NURBS, parameter- and Cartesian-trimmed curves, composite and surface curves, offsets, sweeps, revolutions, curve-bounded surfaces, and geometric sets transfer with their unit and orientation semantics.
- **Topology: Band-wide.** Solid, void, sheet, geometrically bounded surface, oriented-shell, edge-loop, and singular vertex-loop cases transfer through connected body, region, shell, face, loop, coedge, edge, vertex, and pcurve ownership.
- **Tessellation: Band-wide where carried.** AP242 shared coordinate lists, local point-index tables, normals, faces, strips, fans, shells, solids, and exact-body links transfer without duplicating exact solids.
- **Design intent: Inapplicable.** These application protocols exchange solved product shape and structure, not ordered feature/sketch replay history.
- **Product structure: Band-wide.** Product identity, occurrences, mapped items, relative placements, context-dependent transformations, and named external document/resource dependencies transfer.
- **Presentation and metadata: Band-wide.** Layers, direct and overriding styles, colors on topology, exact geometry, tessellation, geometric sets, null styles, semantic dimensions/tolerances/datums, presentation annotations, validation properties, and limits-and-fits classes transfer. Unmodeled application records remain named opaque records with identity and references.
- **Byte accounting: Band-wide.** Every input byte is structural, typed, or part of a named opaque record; unclassified bytes fail the accounting invariant.

The evidence tier is tested. Proven status additionally requires demonstrated coverage across a representative corpus of fixtures for the declared envelope, sustained fuzz runs, and the roadmap's complete round-trip gates.

### Write and round trip

- **Native write: Semantic.** The writer selects AP203 edition 1 or 2, AP214, or AP242 edition 1, 2, or 3 and declares the exact target schema. It emits source-less documents and typed edits for analytic and NURBS geometry, connected solid/sheet/wire topology, pcurves, singular loops, rigid body placements, product occurrences, tessellation, visibility, layers, named colors, and semantic or presentation PMI where the selected application protocol carries them.
- **Procedural geometry: Native where modeled.** Trimmed and spatial-offset curves, linear sweeps, axis revolutions, parallel offsets, and degenerate tori emit as their native STEP entities. Other definitions emit their solved carrier with a machine-readable loss. Curve-bounded surfaces lack the boundary-curve surface association required for valid native regeneration and therefore reduce in report mode or fail strict mode.
- **Fidelity policy: Explicit and atomic.** Report mode writes the representable subset and returns every unsupported semantic fact. Strict mode rejects before writing any byte. Retained opaque records and opaque presentation targets take the refusal path; they are never silently discarded. AP-specific tessellation and PMI compatibility is checked against the selected target.
- **Round trip: Tested.** Source-less, edited, schema-targeted, topology, geometry, product, tessellation, presentation, and PMI outputs re-decode to typed IR. The optional [`verify-step-occt.py`](../scripts/verify-step-occt.py) and [`verify-step-gmsh.py`](../scripts/verify-step-gmsh.py) checks accept and transfer generated shape files across all six targets. The evidence remains tested rather than proven until the representative-corpus and sustained-fuzz gates pass. Corpus availability is not a capability criterion.

## Maintaining these profiles

Per-format specifications in [`formats/`](formats/) define byte semantics. Adjacent `*-open-items.md` files contain unresolved fields and structures.

Support profiles describe repository behavior only. A profile changes when code and tests land, and every **Partial** domain must identify its remaining gates here or in the linked open-items document. Claims move to **Complete** only after satisfying the roadmap's corpus evidence and reliability gates.

Ladder scores change only when a per-gate review confirms every gate at the new level and below. A score's headline names the failing gate of the next level. **Tested** requires fixtures exercising the scored gates.
