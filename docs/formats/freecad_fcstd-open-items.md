# FreeCAD `.FCStd`: Open Items

This document lists the parts of the FreeCAD `.FCStd` format that we do not know. The specification `freecad_fcstd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## Decision queue

These items have a Conflict part and need a decision.

- LP-07. Schema aliases and duplicate section selection
- AR-03. Typed geometry side-entry cardinality
- GP-04. Topology-color shape-property association
- GP-05. GUI provider and property duplicate selection
- PG-01. `ObjectDeps` framing and uniqueness
- PG-03. Property identity and runtime-family selection
- PT-01. `StringHasher2` association
- PT-02. Element-map position to neutral-occurrence order
- PT-03. Element-map carrier and owner selection
- XT-01. Edge endpoint child selection
- XT-02. Edge representation selection and uniqueness
- XT-03. Non-manifold radial order
- DP-01. Forward declared dependencies
- DP-02. Sketch profile seed order
- DP-03. Sketch profile junction ambiguity and tolerance
- DP-04. Design runtime and sketch-carrier dispatch
- PR-01. Product membership and record identity selection
- SA-01. Runtime-type to annotation-kind mapping
- SA-02. Annotation scalar and position property selection
- DG-01. TechDraw runtime-type classification
- DG-02. Drawing link and parameter selection
- BR-01. Text B-rep header and table selection
- AT-01. Attachment frame carrier precedence
- JN-01. Joint kind and enumeration carrier selection
- AG-01. Kernel-property runtime dispatch

## 1. Legacy persistence

### LP-07. Schema aliases and duplicate section selection

**Question.** Which schema attribute spelling and declaration/data sections are authoritative, and are duplicate sections valid?

**Known.** FreeCAD document persistence uses `SchemaVersion` to select the envelope. A schema envelope has one declaration section and one data section.

**Conflict.** `crates/cadmpeg-codec-freecad/src/persistence.rs:39-64` accepts either `SchemaVersion` or `schemaVersion`, chooses the first when both are present, and takes the first matching declaration and data section. Conflicting aliases or duplicate sections can make source order select one graph while silently discarding another.

**Need.** We must establish exact attribute spelling, section cardinality, and duplicate handling from FreeCAD source and malformed fixtures. Conflicting or duplicate structural carriers must be rejected rather than selected by order.

**Note.** The first-candidate paths are direct; the producer's malformed-input policy remains unverified.

## 2. Auxiliary records

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` states that an entry gets semantic meaning from a typed reference in `Document.xml` or `GuiDocument.xml`. An unreferenced entry remains a named archive record. Application data without a neutral representation retains its owning object and property.

**Need.** We must know the framing to parse and validate record boundaries in these side entries.

**Note.** Commit `3d3bf58f4` added an opaque-retention policy and promoted it to the specification. This is a safe decoder policy, not evidence that no FreeCAD side-entry grammar exists. The closure cited no FreeCAD writer path and no saved witness for the framing. Keep the unknown open and retain the opaque fallback.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no typed property grammar identifies the family?

**Known.** The native record retains the owning object, property, declared application type, links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** We must know the field meanings to transfer the side entry to a typed native or neutral record.

**Note.** Commit `3d3bf58f4` converted the absence of a typed decoder into a specification claim that no application record family exists. Opaque retention prevents an unsafe interpretation but does not establish field semantics. The closure did not trace the FreeCAD writer path for these records.

### AR-03. Typed geometry side-entry cardinality

**Question.** How many side entries can one `PropertyMeshKernel` or `PropertyPointKernel` property reference, and which entry contains the geometry payload?

**Known.** The current specification defines one typed payload per property. Property records retain every side-entry request in source order.

**Conflict.** `crates/cadmpeg-codec-freecad/src/application_geometry.rs:31-38` rejects more than one side entry and otherwise reads the first entry. The rejection test only exercises synthetic malformed XML; it does not establish the producer cardinality or entry selection rule.

**Need.** We must establish the cardinality rule for both runtime types. The decoder must reject an invalid cardinality or identify the payload entry from the typed value grammar.

**Note.** Commit `2ceb8c2b0` turned the one-entry policy into settled format prose without a FreeCAD-saved witness or producer source for the cardinality.

### AR-04. Shared side-entry logical ownership

**Question.** How does the logical byte ledger represent one archive entry that is referenced by more than one property or typed payload?

**Known.** `EntryRecord.referenced_by` now retains multiple semantic references while the byte span has one archive-entry owner.

**Need.** We must know whether typed side entries can be shared. If sharing is valid, the ledger needs a separate many-owner relation that does not duplicate byte spans. If sharing is invalid for a typed family, decoding must reject the conflicting claims.

**Note.** Commit `a5882797a` fixed the internal representation but did not establish whether sharing is valid in FreeCAD output. An implementation choice is not evidence for the format rule.

## 3. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when `freecad_fcstd.md` does not define that type?

**Known.** Undefined GUI properties retain their owner, runtime type, status, ordered value elements, side-entry references, exact XML, and byte range.

**Need.** We must know each grammar to parse and validate the property as a typed presentation value.

**Note.** Commit `6d9430a69` established exact handling for selected material and color-list types but closed this broader item by retaining every other type. That fallback avoids guessing; it does not prove the grammar of the remaining GUI types. The closure did not derive the runtime registry from FreeCAD source or from saved witnesses.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when `freecad_fcstd.md` does not define that type?

**Known.** GUI records retain view-provider identity and each undefined property's runtime type and ordered values.

**Need.** We must know the value semantics to transfer the property to the correct neutral presentation field.

**Note.** The opaque/native fallback in `6d9430a69` is not semantic evidence. The item remains open for any unregistered GUI type; the FreeCAD source has not been examined to establish whether the type has a neutral meaning.

### GP-04. Topology-color shape-property association

**Question.** Which exact-shape property and element map does a `DiffuseColor`, `LineColorArray`, or `PointColorArray` side entry describe when the application object owns more than one shape property?

**Known.** Persistent element names supply the neutral topology occurrences that receive an override. Missing identity must leave the side entry retained without guessing a transient topology label.

**Conflict.** `crates/cadmpeg-codec-freecad/src/gui.rs:1289-1314` selects the first application property named `Shape`, the first matching shape payload, the first element map, the last map node, and the first requested topology group. It does not verify that the GUI array and shape property have a persisted association.

**Need.** We must find the persisted association rule between a GUI topology-color array and its application shape property. If the format supplies no association, neutral transfer must require one unambiguous shape candidate.

**Note.** Commit `472740f40` wrote the `Shape` association into the specification and added synthetic count tests. Those tests construct the association expected by the implementation; they do not show that FreeCAD writes it or that duplicate shape candidates are invalid.

### GP-05. GUI provider and property duplicate selection

**Question.** What cardinality and precedence rules govern `ViewProviderData`, provider `Properties`, property value elements, and duplicate GUI property names?

**Known.** The GUI graph retains provider and property source order. Registered properties have name/type pairs.

**Conflict.** `crates/cadmpeg-codec-freecad/src/gui.rs:103-117` takes the first `ViewProviderData` container. At `:141-165`, the neutral property map takes one value element per name and stores duplicate names in a `HashMap`; later entries overwrite earlier ones. At `:401-471`, native projection finds the first same-name property and its first value. The two paths can therefore disagree.

**Need.** We must establish GUI container cardinality, property-name uniqueness, and value precedence from FreeCAD output. Conflicting duplicate records must be rejected or retained without a source-order choice.

**Note.** The selection paths and the disagreement are present in the code read; valid producer cardinality is still unknown.

## 4. Persistence graph

### PG-01. `ObjectDeps` framing and uniqueness

**Question.** What count, ordering, and uniqueness invariants govern the `ObjectDeps` records when the `Objects` section enables dependency records?

**Known.** FreeCAD's writer emits one `ObjectDeps` record for each object, with one `Dep` count per record. The dependency records precede object declarations in the document envelope.

**Conflict.** `crates/cadmpeg-codec-freecad/src/persistence.rs:82-138` validates dependency names and child counts, and `:185-193` validates the filtered dependency order. It does not require the physical `ObjectDeps` block to precede every `Object` element. A dependency block after an object can pass the filtered ordinal checks.

**Need.** The current schema must validate dependency-record count, child count, object coverage, name uniqueness, physical ordering, and `AllowPartial` retention before it builds the object graph.

**Note.** Commit `40fa0adbd` added synthetic framing tests and changed the specification, but the decoder still accepts a source-order violation. The official FreeCAD reader/writer establishes the ordering; the implementation does not enforce all of it.

### PG-03. Property identity and runtime-family selection

**Question.** Are property names unique within one owner, and which exact runtime type selects a property family when names or type tokens conflict?

**Known.** `persistence.rs` retains property order, name, exact type, values, and raw XML. Neutral projections look up properties by owner and name.

**Conflict.** `crates/cadmpeg-codec-freecad/src/persistence.rs:346-477` checks `Properties.Count` but does not reject duplicate `Property` names. The generated native ids at `:382` and `:454` collide for duplicate names. `classify_property` at `:669-721` dispatches by ordered substring tests, so a custom type containing multiple family tokens receives the first family. Downstream `.find` calls then select one duplicate by source order.

**Need.** We must establish FreeCAD property-name uniqueness and the exact runtime-type registry. A conflicting duplicate or multi-family type must be rejected or retained without a semantic family choice.

**Note.** The duplicate identity collision and ordered substring dispatch are direct code paths. The producer invariant is not yet established from the FreeCAD writer path or from authored malformed witnesses.

## 5. Persistent topology identity

### PT-01. `StringHasher2` association

**Question.** Which `StringHasher2` element supplies the data for a `StringHasher new="1"` marker?

**Known.** FreeCAD writes `StringHasher2` immediately after the compatibility marker and restores it directly after the marker.

**Conflict.** `crates/cadmpeg-codec-freecad/src/element_map.rs:45-54` skips the immediate sibling and searches all later element siblings. An interleaved element can make the marker claim a later table.

**Need.** The decoder must require the direct successor that the grammar defines. A malformed association must not shift every later string-table index.

**Note.** The official `StringHasher.cpp` source supports the direct-successor rule. Commit `1bf156d3c` added a test but the current implementation still searches later siblings, so the closure is reopened.

### PT-02. Element-map position to neutral-occurrence order

**Question.** What exact relation connects each final element-map name position to neutral topology occurrences, including repeated placed roots?

**Known.** Persistent names and source topology indices must bind to each placed neutral occurrence. Transient table indices do not constitute persistent identity.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1535-1581` reconstructs a source index with a custom depth-first walk and `or_insert_with`. It does not read an element-map index or cite a FreeCAD/OCCT enumeration rule. Repeated or equal transformed occurrences can collapse to the first key and later traversal changes the assigned index.

**Need.** We must establish the B-rep indexed-map enumeration rule and carry that index through exact-topology transfer. Repeated placements must bind by placement plus source index, not by an inferred traversal.

**Note.** Commit `cfdcda41e` replaced the earlier modulo join and passed repeated-root synthetic tests. The tests verify the new internal walk, not the producer's indexed-map order. The specification now promotes that walk to settled behavior without independent evidence.

### PT-03. Element-map carrier and owner selection

**Question.** Which `Part`, `ElementMap2`, and property carrier belong to one persistent element map when a shape XML contains more than one candidate?

**Known.** Element maps are associated with a shape property and retain their source XML and map order.

**Conflict.** `crates/cadmpeg-codec-freecad/src/element_map.rs:94-120` takes the first `Part` and first `ElementMap2` descendant for each property. `owning_property` at `:208-214` takes the first enclosing property. With two shape payloads or map nodes, source order selects the map used for every persistent-name binding.

**Need.** We must establish the exact element-map carrier cardinality and property association. Duplicate candidates must be rejected or linked by a producer-defined discriminator.

**Note.** The first-candidate paths are direct; the FreeCAD writer path for duplicate carriers has not been traced.

## 6. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation combinations define the start and end vertices of normal, closed, degenerate, and malformed edge records?

**Known.** Exact-shape records retain the complete ordered and oriented topology graph. Neutral edges require explicit start and end vertex identities.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1674-1688` uses `rfind` for the last `Forward` and last `Reversed` child. It rejects only when one orientation is absent. Duplicate orientations silently select the last child; a valid endpoint rule for that case is not established.

**Need.** We must define the valid endpoint child forms. The decoder must handle each valid form explicitly and reject a form that cannot establish both endpoint identities.

**Note.** Commit `63d07acec` wrote the last-child rule into the specification and tested that rule with synthetic records. Duplicate-orientation precedence has not been traced in FreeCAD/OCCT source or read from a saved witness.

### XT-02. Edge representation selection and uniqueness

**Question.** When an edge has multiple 3D curve, polygon, or matching curve-on-surface representations, which representation supplies its neutral carrier and face pcurve?

**Known.** Exact-shape records retain all geometry carriers, locations, parameter ranges, and pcurves. Polygon transfer is a fallback when an exact 3D curve is absent.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:912-928` takes the first kind-1 curve or first kind-5 through kind-7 polygon. `face_pcurve` at `:1223-1251` takes the first matching kind-2 or kind-3 representation. Neither path checks duplicate equivalence.

**Need.** We must establish representation cardinality and precedence. If multiple candidates are legal, the decoder must select by serialized role or require equivalent geometry; otherwise it must reject the duplicate form.

**Note.** Commit `63d07acec` promoted source order to the specification; the FreeCAD/OCCT writer path for multiple representations was not traced.

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same edge?

**Known.** Native topology retains ordered child uses and orientations. A neutral coedge has one `radial_next` relation.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1661-1671` links one or two coedges, but leaves three or more self-radial. It therefore selects “no radial order” without showing that the source has no radial relation.

**Need.** We must establish whether the B-rep topology supplies a radial order for non-manifold uses. If it does not, the neutral model must retain unordered incidence or mark the radial order unresolved.

**Note.** Commit `63d07acec` changed the neutral fallback and stated that the source has no radial order. The closure cited no writer path and no non-manifold witness.

## 7. Design projection

### DP-01. Forward declared dependencies

**Question.** Can a declared `ObjectDeps` target appear later than its dependent object in source order?

**Known.** Declared dependencies and earlier link-property operands form the feature dependency graph. The earlier-source restriction applies to link operands, not declared dependencies.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:380-400` filters every dependency by the target feature ordinal being earlier than the consumer. A declared forward dependency remains native but disappears from the neutral feature graph.

**Need.** The decoder must preserve every resolved declared feature dependency. It must apply the earlier-source rule only to link-property operands.

**Note.** The specification in commit `cc7953ac4` says forward declared dependencies are legal, but the current filter still drops them. This is a direct implementation/spec conflict.

### DP-02. Sketch profile seed order

**Question.** Which non-construction entity starts each oriented sketch profile chain?

**Known.** Sketch entities retain persisted source order and native identity. Profile chains must be deterministic and attributable.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:2260-2304` selects the smallest in-memory entity index. The current code no longer uses lexicographic decimal ids, but the seed rule for multiple disconnected chains has not been traced in FreeCAD source.

**Need.** Profile construction must keep the persisted entity ordinal as data and use a producer-defined seed rule for each chain.

**Note.** Commit `cc7953ac4` fixed the decimal-string ordering defect and added synthetic profile tests. The tests do not establish the source rule for disconnected profiles, so the item remains open.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What endpoint tolerance connects two sketch entities, and what happens when more than one unused entity meets the current endpoint?

**Known.** Constraints and persisted geometry can produce coincident endpoints. A neutral profile chain asserts one ordered continuation and orientation at every junction.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:2269-2349` uses coordinate proximity in addition to coincident constraints and takes the first remaining candidate during chain growth. `near` at `:2427-2434` uses `64 * f64::EPSILON * max_coordinate_scale`; no FreeCAD tolerance or admissible profile topology supports this value.

**Need.** We must establish the endpoint equivalence rule and the admissible profile topology. An ambiguous junction must use constraint identity, an explicit source order rule, or an attributable refusal instead of a first match.

**Note.** Commit `e024f02dd` added ambiguity handling and a scale formula, but the boundary still rests on an uncited constant. Exact and synthetic ambiguous cases do not verify the numeric boundary.

### DP-04. Design runtime and sketch-carrier dispatch

**Question.** Which exact runtime type and child value select a design feature or sketch geometry family?

**Known.** Native records retain the exact runtime type and ordered XML children. Known FreeCAD families have family-specific value fields.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:380-395` selects Fillet or Chamfer by substring. `parse_sketch` at `:1009-1029` takes the first eligible carrier child. `sketch_geometry` at `:2104-2251` selects the first matching substring family, with `Arc`, `Ellipse`, and `Circle` checks ordered by the decoder. A vendor type or multi-carrier value can enter the wrong semantic family or discard a later carrier.

**Need.** We must establish the exact design runtime registry, child grammar, and family precedence. Unknown or conflicting carriers must remain native or be rejected.

**Note.** The alternative order and first-child choices are direct code paths; valid extension naming and carrier cardinality are unknown.

## 8. Product structure

### PR-01. Product membership and record identity selection

**Question.** What cardinality and precedence rules govern repeated product records, overlapping container membership, and linked prototype records?

**Known.** Product records retain object identity, source order, members, prototypes, and transforms. Neutral occurrences need one parent and one resolved prototype transform.

**Conflict.** `crates/cadmpeg-codec-freecad/src/product.rs:165-170` uses `or_insert` and assigns the first container as parent. `:253-256` builds `record_by_object` without duplicate rejection, so the last duplicate wins. `:393-400` resolves the first matching prototype record. `product_kind` at `:580-594` uses ordered substrings. These choices can disagree for the same object.

**Need.** We must establish the producer cardinality and precedence for container membership, product records, prototype links, and runtime types. Ambiguous records must be rejected or retained without a source-order choice.

**Note.** The first/last choices and their inconsistent maps are direct; the valid FreeCAD cardinalities are not established.

## 9. Semantic annotations

### SA-01. Runtime-type to annotation-kind mapping

**Question.** Which exact application runtime types represent dimensions, geometric tolerances, datums, balloons, leaders, symbols, and text annotations?

**Known.** The native annotation record retains the exact runtime type. The neutral arena requires separate semantic kinds.

**Conflict.** `crates/cadmpeg-codec-freecad/src/annotation.rs:162-220` now uses an exact core registry and keeps other types native. The registry is supported by positive fixture cases, but no exhaustive FreeCAD source or inheritance rule establishes that every unlisted type is non-semantic.

**Need.** We must define an exact or inheritance-aware runtime-type registry and its kind mapping. Unknown application annotation types must remain native until their semantic family is established.

**Note.** Commit `57371f57d` added exact negative tests and a real TechDraw fixture, but positive coverage of listed types does not verify registry completeness. The former substring guess was narrowed, not proven exhaustive.

### SA-02. Annotation scalar and position property selection

**Question.** Which property carries the semantic scalar and position for each annotation runtime type?

**Known.** The native property graph retains every named value independently. A neutral semantic annotation has one optional scalar and one optional position.

**Conflict.** `crates/cadmpeg-codec-freecad/src/annotation.rs:243-311` selects the first available property in the fixed scalar order `Value`, `Measurement`, `Distance`, `Angle`, and selects `Position` before an `X`/`Y` pair. Duplicate same-named properties and values are not rejected.

**Need.** We must map scalar and position carriers by runtime type and reject contradictory duplicate carriers. The decoder must not use property-name priority as semantic dispatch.

**Note.** Commit `57371f57d` improved runtime-type filtering but retained first-property and first-value selection. The exact property registry and precedence remain unverified.

## 10. TechDraw projection

### DG-01. TechDraw runtime-type classification

**Question.** Which exact runtime types enter the TechDraw arena and which drawing kind does each type represent?

**Known.** Drawing records retain the exact runtime type and source order.

**Conflict.** `crates/cadmpeg-codec-freecad/src/drawing.rs:23-25` admits every type containing `TechDraw::`. `classify` at `:140-167` chooses the first matching substring among Page, Template, Dimension, Annotation, Balloon, Leader, Symbol, Detail, Section, Projection, Image, and View. A vendor type containing a known token, or a type containing two tokens, receives decoder-defined semantics.

**Need.** We must establish the exact TechDraw runtime registry and inheritance/type mapping. Unknown extension types must remain native.

**Note.** The filter and ordered classifier are direct; valid extension type names are not established.

### DG-02. Drawing link and parameter selection

**Question.** What cardinality and precedence rules select drawing templates, scalar/vector carriers, link properties, and repeated parameters?

**Known.** Drawing records retain all links and source properties.

**Conflict.** `crates/cadmpeg-codec-freecad/src/drawing.rs:34-50` takes the first `Template` link. `scalar_property` and `vector_property` at `:170-193` take the first same-name property and first value. `links` at `:196-202` ignores later same-name properties. `drawing_parameters` at `:204-230` stores duplicate names in a `BTreeMap`, so the later value wins. Reordering conflicting carriers changes the projected drawing.

**Need.** We must establish the TechDraw property definitions, cardinalities, and precedence from FreeCAD source or saved witness documents.

**Note.** The code has separate first-wins and last-wins paths; producer uniqueness remains unverified.

## 11. Text B-rep

### BR-01. Text B-rep header and table selection

**Question.** What uniqueness and framing rules govern the text B-rep topology header and section markers?

**Known.** The parser requires one supported topology version and the ordered tables `Locations`, `Curve2ds`, `Curves`, `Polygon3D`, `PolygonOnTriangulations`, `Surfaces`, `Triangulations`, and `TShapes`.

**Conflict.** `crates/cadmpeg-codec-freecad/src/brep.rs:736-749` selects the first header marker found by whole-text `contains` checks. At `:753-782` and `:784-787`, each section count comes from the first token occurrence. A concatenated or embedded payload with repeated markers can select an earlier version or table while later markers carry the actual records.

**Need.** We must establish the producer framing and reject duplicate or embedded header/table markers instead of accepting the first occurrence.

**Note.** Valid OCCT text B-rep output normally has one header and one table, but the decoder has no uniqueness check for repeated markers.

## 12. Attachment and assembly

### AT-01. Attachment frame carrier precedence

**Question.** How do `Placement` and `AttachmentOffset` combine when both are present, and which property/value is authoritative when repeated?

**Known.** Attachment records retain support, map mode, placement, offset, and an effective frame.

**Conflict.** `crates/cadmpeg-codec-freecad/src/attachment.rs:23-39` assigns `effective_frame = placement.or(offset)`, so `AttachmentOffset` is ignored whenever `Placement` exists. The property helper at `:23-27` and value helper at `:45-53` also take first matches. Two valid carriers can therefore produce a different neutral frame after source reordering.

**Need.** We must establish the FreeCAD attachment composition and property cardinality. The decoder must compose or reject conflicting carriers according to that rule.

**Note.** The precedence is explicit; the FreeCAD composition rule for the effective frame has not been traced.

### JN-01. Joint kind and enumeration carrier selection

**Question.** Which joint property and value carrier define the joint kind when `ObjectToGround`, `JointType`, repeated values, or both integer and enum values are present?

**Known.** Joint records retain the source properties, links, placements, offsets, and parameter values.

**Conflict.** `crates/cadmpeg-codec-freecad/src/joint.rs:29-36` gives `ObjectToGround` precedence over `JointType`. `enumeration_value` at `:266-283` takes the first `Integer` and the nth `Enum`; property and parameter helpers also take first matches. A record with both grounded and joint-type carriers, or duplicate enumeration values, changes kind and operands with source order.

**Need.** We must establish joint runtime grammars, carrier cardinality, and grounded/joint-type precedence from FreeCAD source or saved witness documents.

**Note.** The selection is direct, but the producer may forbid the conflicting forms.

## 13. Typed application geometry

### AG-01. Kernel-property runtime dispatch

**Question.** Which exact runtime types select mesh or point-kernel decoding?

**Known.** Mesh and point side entries have different binary payload grammars and neutral arenas.

**Conflict.** `crates/cadmpeg-codec-freecad/src/application_geometry.rs:25-56` uses substring checks and tests `PropertyMeshKernel` before `PropertyPointKernel`. A custom or compound runtime type containing both tokens is decoded as mesh; its point payload is not considered.

**Need.** We must establish the exact runtime-type registry and value grammar. Unknown or multi-family types must remain opaque or be rejected.

**Note.** The dispatch order is direct; the exact runtime-type registry has not been traced in FreeCAD source.
