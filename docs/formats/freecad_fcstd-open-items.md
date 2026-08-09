# FreeCAD `.FCStd`: Open Items

This document lists the parts of the FreeCAD `.FCStd` format that we do not know. The specification `freecad_fcstd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Legacy persistence

### LP-01. Schema 2 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 2 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 2 object boundary and value.

### LP-02. Schema 2 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 2 property boundary and value.

### LP-03. Schema 3 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 3 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 3 object boundary and value.

### LP-04. Schema 3 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 3 property boundary and value.

### LP-05. Legacy object-layout dispatch

**Question.** Which version fields and type fields select each pre-schema-4 object layout?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that a decoder must identify the governing version before it refuses an unsupported layout. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." lists the structural attributes that select parsing dispatch.

**Need.** We must know the selection rule to choose the correct object grammar before object decoding starts.

### LP-06. Legacy property-encoding dispatch

**Question.** Which version fields, property types, and value tags select each property encoding before schema 4?

**Known.** `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that document schema, file version, property type, and value tag select parsing dispatch.

**Need.** We must know the selection rule to choose the correct property grammar before property decoding starts.

## 2. Auxiliary records

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that an entry gets semantic meaning from a typed reference in `Document.xml` or `GuiDocument.xml`. An unreferenced entry remains a named archive record. `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" defines exact retention for application data without a neutral representation.

**Need.** We must know the framing to parse and validate record boundaries in these side entries.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" requires retention of the owning object, property, declared application type, links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** We must know the field meanings to transfer the side entry to a typed native or neutral record.

## 3. Sketch geometry

### SG-02. Conic conventions without a FreeCAD-saved witness

**Question.** Do FreeCAD-saved documents give the elliptical-arc, hyperbola, and parabola carriers the parameterization that the decoder applies?

**Known.** FreeCAD source (`Save` and `Restore` for `GeomEllipse`, `GeomArcOfEllipse`, `GeomArcOfHyperbola`, and `GeomArcOfParabola` in `src/Mod/Part/App/Geometry.cpp`) defines the convention. `AngleXU` is the counterclockwise angle from the sketch X axis to the major axis. `StartAngle` and `EndAngle` are OCCT curve parameters. For a bounded ellipse, the point at parameter `t` is `center + major_radius * cos(t) * major_direction + minor_radius * sin(t) * minor_direction`, where the minor direction is the major direction rotated by a quarter turn. The decoder's endpoint evaluation applies the same formula. No fixture in the corpus is a FreeCAD-saved document that carries a bounded elliptical arc, a rotated ellipse, a hyperbola, or a parabola, so no test compares the decoder against bytes that FreeCAD wrote.

**Need.** A FreeCAD-saved document with this geometry gives an independent witness for the convention. Agreement between the decoder and a source reading can hide one shared misreading. A saved document removes that possibility, and its absence keeps the conic decode arms unproven against real output.

### SG-03. FreeCAD-saved conic fixture authoring

**Question.** Which FreeCAD-saved fixtures must the corpus contain to cover conic sketch geometry and profile chaining?

**Known.** `corpus/freecad_fcstd/author_fixtures.py` authors fixtures through the FreeCAD Python API on a machine with a FreeCAD installation. The current build machine has no FreeCAD installation. `corpus/freecad_fcstd-fixture-charter.md` states that a synthetic parser input establishes no ladder score. No current fixture carries an arc with a non-zero sweep that a profile chain consumes.

**Need.** We must extend the authoring script with one sketch that contains a circular arc with a non-zero `AngleXU`, a bounded elliptical arc with a rotated major axis, a full rotated ellipse, a hyperbola, a parabola, and line segments that meet the arc endpoints. The rotation angles must stay away from multiples of a quarter turn, because at those angles a frame built with swapped axes produces the same points. We must then run the script under FreeCAD, commit the saved documents, and pin their decode, inspect, encode, and STEP goldens. These fixtures resolve SG-02, and they can support format-support claims because FreeCAD wrote them.

## 4. Persistent topology identity

### PT-02. Element-map position to neutral-occurrence order

**Question.** What exact relation connects each final element-map name position to neutral topology occurrences, including repeated placed roots?

**Known.** `freecad_fcstd.md` §7 "A newly encoded element map likewise uses a compatibility marker" states that group order and name position establish the transient `Face1`, `Edge1`, and `Vertex1` indices. Those positions connect persistent names to every placed neutral occurrence. The source index belongs to the B-rep topology map, not to persistent identity.

**Conflict.** `element_map.rs` `bind_topology` gathers all neutral ids of one kind in arena traversal order. It assigns the Nth id to one-based name position N and repeats the complete sequence for placed occurrences. No source topology index is carried through this join, and no check proves that arena order equals the B-rep indexed-map order for each placement. A different traversal or a missing occurrence can attach a valid persistent name to the wrong face, edge, or vertex.

**Need.** We must establish the B-rep indexed-map enumeration rule and carry that index through exact-topology transfer. Repeated placements must bind by placement plus source index, not by a global modulo assumption.

## 5. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation combinations define the start and end vertices of normal, closed, degenerate, and malformed edge records?

**Known.** Exact-shape records retain the complete ordered and oriented topology graph. Neutral edges require explicit start and end vertex identities.

**Conflict.** `topology_transfer.rs` `ensure_edge` searches for a `Forward` child and a `Reversed` child. If either search fails, it uses the first child; if the reversed search still has no value, it duplicates the selected start. Thus a record with a missing orientation can become a closed edge, and an unrelated first child can become an endpoint. No loss or refusal identifies the substitution.

**Need.** We must define the valid endpoint child forms. The decoder must handle each valid form explicitly and reject a form that cannot establish both endpoint identities.

### XT-02. Edge representation selection and uniqueness

**Question.** When an edge has multiple 3D curve, polygon, or matching curve-on-surface representations, which representation supplies its neutral carrier and face pcurve?

**Known.** `freecad_fcstd.md` §7 "Part shape properties reference text or binary B-rep entries." requires retention of all geometry carriers, locations, parameter ranges, and pcurves. Polygon transfer is a fallback for an edge without an exact 3D curve.

**Conflict.** `topology_transfer.rs` `ensure_edge` takes the first kind-1 curve representation, or the first kind-5 through kind-7 polygon representation. `face_pcurve` takes the first kind-2 or kind-3 representation whose surface and transform match. Neither path checks uniqueness or equivalence among multiple accepted candidates. Record order therefore selects the neutral geometry and parameter range.

**Need.** We must establish representation cardinality and precedence. If multiple candidates are legal, the decoder must select by serialized role or require equivalent geometry; otherwise it must reject the duplicate form.

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same edge?

**Known.** The native topology retains ordered child uses and their orientations. A neutral coedge has one `radial_next` relation. For a manifold edge, the one- or two-use cycle has no additional ordering choice.

**Conflict.** `topology_transfer.rs` `close_radial_rings` groups emitted coedges by edge and links them in global emission order. For three or more uses, this asserts a radial order without reading a source relation or deriving the around-edge order from geometry. A different root or face traversal changes the asserted cycle.

**Need.** We must establish whether the B-rep topology supplies a radial order for non-manifold uses. If it does not, the neutral model must retain an unordered incidence relation or mark the radial order as unresolved.
