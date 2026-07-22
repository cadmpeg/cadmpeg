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
| Rhino `.3dm` (archive 50/60/70/80)         | **L9 tested**  |                                                                                                         |
| CATIA V5 `.CATPart` (standard-nested band) | **L2 claimed** | conditionally connected B-rep                                                                         |
| Siemens NX `.prt`                          | **L2 claimed** | conditional connected B-rep, external-dependency inspection                                           |
| CATIA V5 `.CATPart` (other layout bands)   | **L1 claimed** |                                                                                                       |
| Creo Parametric `.prt`                     | **L1 claimed** | partial placed geometry, connected topology, sketches, constraints, parameters, expressions, features |
| Rhino `.3dm` (V3/V4)                       | **L1 tested**  | metadata and bounded object-record retention                                                          |
| Rhino `.3dm` (V1/V2 and archive 5)         | **L0 tested**  | header-only inspection; decode is rejected                                                            |
| STEP Part 21 AP242 editions 1–3            | **L9 tested**  |                                                                                                         |
| STEP Part 21 AP203 editions 1–2 and AP214  | **L9 tested**  |                                                                                                         |
| IGES 5.3 Fixed ASCII mechanical/document   | **L8 tested**  | read only                                                                                               |

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
- **Siemens NX `.prt` (L2 claimed):** exact carriers and conditionally connected topology. Read only.
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

**Ladder: L4 tested.** Unresolved procedural families and unknown carriers block L5. Native writing exceeds the scored gates.

### Read profile

- **Container and versions: Partial.** The codec selects the first `.smbh` entry, falling back to the first B-rep entry, and decodes linked Protein, Design, MetaStream, and ACT records. The authoritative relation among multiple asset folders and B-rep entries remains unresolved.
- **Geometry: Partial.** Analytic surfaces and curves, cached NURBS carriers, parameterizations, signed radii, point-degenerate curves, exact, compound, ruled, sum, revolution, offset, rolling-ball, pipe, taper-family, loft, and G2-blend spline surfaces, exact-cache curve, compound/deformable curves, helix, vector-offset, surface-offset, spring, subset, projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, and prefix-only blend/surface/parametric/skin constructions with null, analytic, or NURBS surface/UV support pairs transfer under both modern and legacy subtype names. Law-dependent net/skin/sweep forms, variable/vertex blends, and related families remain incomplete when no solved cache resolves.
- **Topology: Partial.** Shell-reachable solid, sheet, wire, and mixed-dimensional general bodies transfer with shells, faces, loops, coedges, wire edges, edges, and vertices. Closed edge incidence classifies solid bodies, open face incidence classifies sheet bodies, face-less wire membership classifies wire bodies, and bodies containing both faces and wire edges classify as general. Unsupported surface records retain topology with unknown geometry; free vertices, some procedural edges, and some explicit pcurves remain unresolved.
- **Tessellation: None.** Fusion display meshes remain outside the IR tessellation arena.
- **Design intent: Partial.** ASM history states, Design assignments, sketch-side records, construction recipes, persistent references, MetaStream identities, and ACT channels transfer. A complete replayable Fusion feature history remains open.
- **Product structure: Partial.** Body transforms and root-component records transfer. Multi-component structure and constraints remain open.
- **Presentation and metadata: Partial.** Linked source attributes, Protein appearance assets, material properties, body bindings, and per-body display visibility transfer. External material-library display names and some schema fields remain unresolved.

### Write and round trip

- **Native write: Partial.** An unchanged retained source archive writes byte for byte. The writer patches model points, common analytic and NURBS B-rep curves and surfaces, rational/non-rational pcurves, procedural caches, sketch geometry, constraints, history fields, design records, and supported appearance properties in their original records. Source-less generation writes multi-body B-reps containing solid, sheet, wire, and mixed-dimensional general topology with analytic or rational/non-rational NURBS carriers; exact, compound, ruled, sum, revolution, offset, rolling-ball, taper, loft, G2-blend, and translational-extrusion surface constructions; exact-cache, compound, deformable, helix, vector-offset, and subset curve constructions; inline rational/non-rational NURBS pcurves; placements; document tolerance metadata; direct body/face color attributes; body/face/edge persistent-design tags; coedge-to-sketch provenance; typed ASM history; Design object and construction streams; sketch geometry and constraints; ACT tables and component roots; and Protein appearances with body bindings.
- **Write limits:** General writing requires a retained source archive and the original entity and record layouts. Source-less generation supports multiple placed bodies, regions, and shells with plane, cylinder, cone, sphere, torus, or rational/non-rational NURBS faces; multiple loops; shared radial edges; line, circle, ellipse, point-degenerate, or rational/non-rational NURBS edge curves; inline rational/non-rational NURBS pcurves; face-less wire bodies with chained regions and shells; and mixed face-and-wire shells with one or more wire edges. Free vertices and edits outside the listed fields are rejected.
- **Procedural write invariant:** One solved curve has at most one procedural construction. Exact-cache, compound, deformable, helix, vector-offset, surface-offset, spring (including conditional null-carrier ranges), subset, ranged and early-close projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, and prefix-only blend/surface/parametric/skin definitions with paired analytic/NURBS surface and NURBS UV supports serialize with their native construction fields, child carriers, role fields, and solved caches. Scalar offset and blend-spine definitions remain rejected source-less until their external fields can be emitted without semantic loss. No typed construction is reduced silently to a cache-only curve.
- **Round trip: Partial.** Generated byte fixtures cover byte-exact replay and each writable geometry, history, Design, sketch, ACT, and appearance family.

See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).

## Siemens NX `.prt`

**Kernel:** Parasolid in an SPLMSSTR container

**Ladder: L2 claimed.** L3 requires topology across the band. Supported adjacent equal-schema partition/deltas pairs apply exact-key replacements and tombstones; unmatched tombstones and remaining record families still prevent a band-wide topology claim.

### Read profile

- **Container and versions: Partial.** The codec decodes the SPLMSSTR directory and extracts and classifies embedded Parasolid partition, deltas, and related streams.
- **Geometry: Partial.** Points, analytic surfaces and curves, typed B-spline surfaces and curves, and supported type-133 trimmed curves transfer into IR.
- **Topology: Partial.** The body, shell, face, loop, fin, edge, and vertex graph attaches when framing and references resolve. Exact-key BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, B_SURFACE, and B_CURVE deltas replacements and tombstones merge in source order for adjacent equal-schema pairs. Unmatched tombstone relations remain unresolved.
- **Tessellation: None.**
- **Design intent: Partial.** Typed numeric expressions retain object identity, name, declared millimeter or degree unit, and value. Named arrangements retain ordered configuration names and default state. Feature, sketch, constraint, and history operation semantics remain open.
- **Product structure: Inspect.** The codec reports external part dependencies. Assembly graph instances, placements, and constraints remain open.
- **Presentation and metadata: None.**

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open geometry gates include unresolved procedural-intersection branches, freeform NURBS-offset blend spines, and other unsupported record families. Open structural gates include unmatched tombstones, multi-partition feature composition, assembly records, and NX object-model field serialization.

See [`formats/siemens_nx.md`](formats/siemens_nx.md) and [`formats/siemens_nx-open-items.md`](formats/siemens_nx-open-items.md).

## CATIA V5 `.CATPart`

**Kernel:** CGM

**Ladder: L2 claimed (standard-nested band); L1 claimed (other layout bands).** L3 requires connected topology across the band. Current topology depends on resolved trim, support, and endpoint assignments.

### Read profile

- **Container and versions: Partial.** The codec decodes `V5_CFV2` containers and distinguishes standard-nested, FBB-only, zero-entity, float-packed, E5, and inner-without-directory layouts.
- **Geometry: Partial.** Standard-nested files transfer vertices, planes when their bridge records resolve, curved analytic surfaces, and supported edge curves. Other layouts transfer subsets of analytic or freeform carriers.
- **Topology: Partial.** Standard-nested files can emit a connected body, shell, face, loop, coedge, edge, and vertex graph when trim, support, and endpoint assignment resolve. Other parsed topology families remain disconnected from the common IR.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: None.** Persistent tags, attributes, materials, and appearance bindings remain outside the IR.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open gates include endpoint incidence for additional variants, orientation signs, pcurve attachment, spline edge curves, persistent tags, attributes, and the consolidated-stream tag resolver.

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
