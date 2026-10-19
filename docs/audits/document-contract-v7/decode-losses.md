# Decode loss and retained-content audit

This inventory records the per-code decision used to declare `SourceFidelity.unrepresented`. A source record is represented only when its semantic payload survives in a typed neutral or native arena. Source images, raw framed payloads, `unknowns`, `Unreadable` values, names, offsets, and issue records do not represent the missing payload.

The classification applies to the source record described by the loss. It does not infer the document's neutral `scope`. A partial projection holds its class and reports losses. A wholly omitted or failed projection does not hold that class. Typed native records may retain content even when the corresponding neutral class is outside scope.

| Class | Meaning | Fidelity effect |
|---|---|---|
| A | Design source record has no typed representation. | Insert `design`. |
| B | Model source record has no typed representation. | Insert `model`. |
| C | A typed arena retains the content with reduced neutral fidelity. | No insertion for this record. |
| D | Syntax, envelope, or diagnostic information without omitted arena content. | No insertion. |
| (w) | Export-only code. | No decode fidelity effect. |

Conditional entries state the retention test. `A+B` inserts both classes. An `A/C` or `B/C` entry inserts a class only when the stated typed retention is absent. In addition to these loss-driven declarations, an intentionally skipped source projection declares the classes it leaves source-only. This includes container requests and codec-specific skipped design projection. An empty loss list does not authorize replay of skipped source content.

The tables list local wire codes. The namespace is the codec identifier, except FreeCAD uses `fcstd`. The owning implementation is each codec's `src/loss.rs`, with source-population declarations at decode assembly. Golden Full and Geometry decoding checks compare the assembled fidelity declaration with these codec-specific rules; the shared depth harness checks delivered scope, population, and `depth.not-delivered` consistency.

| Codec | Local codes |
|---|---|
| catia | 39 |
| creo | 76 |
| f3d | 109 |
| freecad | 34 |
| iges | 54 |
| inventor | 37 |
| nx | 37 |
| rhino | 53 |
| sat | 18 |
| sldprt | 71 |
| step | 156 |

## CATIA

| Local wire code | Class | Retention decision |
|---|---|---|
| `attributes.dimension-quantity-unresolved` | B/C | Actual values retained in typed native.catia.entity_records; if absent, class B. |
| `attributes.materials-metadata-not-transferred` | B | Materials, unbound NURBS caches, and document metadata were not transferred. The omitted record payload has no typed document representation. |
| `attributes.visualization-unbound` | B | Visualization value blocks lack a proven typed face or body target. The omitted record payload has no typed document representation. |
| `geometry.analytic-payload-invalid` | B | Analytic surface records had a non-finite or out-of-range payload. The omitted record payload has no typed document representation. |
| `geometry.brep-not-transferred` | B | No B-rep geometry was transferred for this storage variant. The omitted record payload has no typed document representation. |
| `geometry.carrier-summary` | D | Verbatim vertex points and analytic surface carriers were decoded. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.face-local-freeform-not-transferred` | C | Face-local native freeform records retain their decoded carrier values and identities. |
| `geometry.line-profile-not-transferred` | B/C | Actual values retained in typed native.catia.consolidated_line_profiles; if absent, class B. |
| `geometry.parameter-range-invalid` | B | A record stated a parameter range that is non-finite or not increasing. The omitted record payload has no typed document representation. |
| `geometry.pcurve-rechart-non-finite` | B | Equivalent support charts produce non-finite pcurve coordinates. The omitted record payload has no typed document representation. |
| `geometry.plane-parameters-invalid` | B | Plane surface records lacked valid tag-bridged parameter records. The omitted record payload has no typed document representation. |
| `geometry.revolution-profile-unbound` | C | Native revolution records retain their typed profile reference; binding is reduced. |
| `geometry.unresolved-carriers` | B | Transferred model retains unresolved curve or surface carriers. The omitted record payload has no typed document representation. |
| `history.configuration-row-order-bounded` | A | Configuration-row links kept no intervening entities at their allowance. The omitted record payload has no typed document representation. |
| `history.legacy-runs-unresolved` | A | Legacy design runs retain unresolved selector, relation, and feature semantics. The omitted record payload has no typed document representation. |
| `history.modeling-scope-unresolved` | A | Outer declarations do not select one physically contained object graph. The omitted record payload has no typed document representation. |
| `history.object-records-unresolved` | A | Modeling-scope object-graph field records remain unresolved. The omitted record payload has no typed document representation. |
| `history.relation-dependencies-bounded` | A | Relations kept no parameter dependencies at their allowance. The omitted record payload has no typed document representation. |
| `source.annotation-identity-collision` | D | Carrier populations assigned conflicting annotations to one identity. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.catalog-scan-bounded` | A+B | The 7C02 catalog scan skipped candidates at its walk allowance. The omitted record payload has no typed document representation. |
| `source.dialect-unverified` | D | The storage layout matched no declared dialect's structural invariants. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.route-fell-through` | B | A decode route refused source records and then transferred no model, so the decode continued to the next route or to the metadata fallback. The omitted record payload has no typed document representation. |
| `topology.b5-gauge-substituted` | C | B5 reference graph is closed; face sense and body kind use a topology gauge. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.b5-graph-unclosed` | B | Object-stream and NURBS carriers decoded, but the B5 graph did not close. The omitted record payload has no typed document representation. |
| `topology.b5-population-bounded` | B | B5 topology runs were read without the isolated geometry they reference. The omitted record payload has no typed document representation. |
| `topology.b5-subset-incomplete` | C | Excluded B5 nodes remain typed native nodes with geometry and incidence fields. |
| `topology.boundary-graph-not-emitted` | B | Detected FBB face rows were not emitted as a B-rep boundary graph. The omitted record payload has no typed document representation. |
| `topology.e5-gauge-substituted` | C | E5 reference graph is closed; orientation uses an incidence-derived gauge. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.e5-graph-unclosed` | B | E5 carriers decoded, but the reference graph did not close. The omitted record payload has no typed document representation. |
| `topology.fbb-rows-withheld` | B | Candidate FBB face rows were withheld from the standard topology population. The omitted record payload has no typed document representation. |
| `topology.graph-not-built` | B | B-rep topology graph was not built for this file. The omitted record payload has no typed document representation. |
| `topology.object-stream-work-slice-exhausted` | B | Object-stream graph exceeded the bounded work slice. The omitted record payload has no typed document representation. |
| `topology.planar-boundary-bounded` | B | A planar face stated too many boundary points to classify its loops. The omitted record payload has no typed document representation. |
| `topology.standard-search-bounded` | B | A standard-family topology evidence search reached its local work bound. The omitted record payload has no typed document representation. |
| `topology.zero-entity-candidates-bounded` | B | Zero-entity endpoint-pair and endpoint-locus candidates exceeded their work bound. The omitted record payload has no typed document representation. |
| `topology.zero-entity-face-unresolved` | B | Zero-entity wire loops transferred; face topology remains unresolved. The omitted record payload has no typed document representation. |
| `topology.zero-entity-gauge-substituted` | C | Zero-entity topology transferred under a derived or source-bound gauge. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.zero-entity-not-transferred` | B | Zero-entity loop members bind supports, but no neutral topology transferred. The omitted record payload has no typed document representation. |
| `topology.zero-entity-supports-retained` | C | Typed native support records retain UV/model carriers and endpoint values. |

## Creo

| Local wire code | Class | Retention decision |
|---|---|---|
| `carrier.analytic-pcurves` | D | Exact analytic carriers transferred from native linear pcurves. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.datum-planes` | D | Exact model-space construction datum planes transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.extrusion-boundary-curves` | D | Exact NURBS boundary carriers transferred at extrusion-plane contacts. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.extrusion-section-generators` | D | Exact NURBS generator carriers transferred from plane-section directrices. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.first-instance-prototypes` | D | First-instance ND analytic or interpolation-spline carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.line-extrusion-planes` | D | Unbound straight positional surface-of-extrusion carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.paired-envelope-spheres` | D | Sphere carriers transferred from complementary hemisphere envelopes. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.positional-cones` | D | Exact positional cone carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.positional-cylinders` | D | Exact positional cylinder carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.positional-tori` | D | Exact positional torus carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.reference-circles` | D | Circular reference carriers transferred from MdlRefInfo. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.reference-ellipses` | D | Elliptical reference carriers transferred from MdlRefInfo. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.reference-lines` | D | Finite model-space reference line carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.shared-extrusion-generators` | D | Exact shared NURBS generator carriers transferred between extrusions. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.tabulated-cylinder-extrusions` | D | Tabulated-cylinder cubic spline extrusion carriers transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.topological-edges` | D | Native topological edges with exact endpoint points transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.topological-points` | D | Exact model-space points transferred for topological vertex orbits. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.topology-bound-planes` | D | Model-space planes transferred from topology-bound constructions. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.torus-parameter-retention` | C | Tagged torus radius, outline, and envelope fields retained as native data. The source record survives as typed content; its neutral fidelity is reduced. |
| `carrier.visibgeom-planes` | D | Model-space planes transferred from VisibGeom local-system frames. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `configuration.driver-unresolved` | A | Referenced configuration driver tables retain unresolved traversal. The omitted record payload has no typed document representation. |
| `container.census` | D | PSB section census and prototype/instance transfer summary. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `container.compressed-section-unexpanded` | A+B | A byte-bounded compressed section has invalid or incomplete LZW data. The omitted record payload has no typed document representation. |
| `container.toc-scan-capped` | A+B | TOC headers past the input length in visited header and row bytes were not read. The omitted record payload has no typed document representation. |
| `curve-expression.kind-prohibited` | C | Prohibited datum-curve constructs across active curve-equation records. The source record survives as typed content; its neutral fidelity is reduced. |
| `curve-expression.prohibited` | C | Active curve-equation records with prohibited constructs were not evaluated. The source record survives as typed content; its neutral fidelity is reduced. |
| `curve-expression.solve-control-unresolved` | C | Active curve-equation records retain malformed simultaneous-solve control. The source record survives as typed content; its neutral fidelity is reduced. |
| `curve-expression.solve-unresolved` | C | Active curve-equation simultaneous-solve blocks retain unsolved equations. The source record survives as typed content; its neutral fidelity is reduced. |
| `curve-expression.text-capped` | A | Curve-equation relation values were dropped at a local string-text cap. The omitted record payload has no typed document representation. |
| `feature.construction-incomplete` | C | Construction history features retain unresolved neutral operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.construction-unresolved` | C | Typed history features retain an explicitly unresolved construction. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.native-semantics` | C | FeatureOperation::Native fallback retains the typed native operation and operands. |
| `feature.neutral-semantics-incomplete` | C | Neutral feature, configuration, graph, material, and display data remain open. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.recognized-incomplete` | C | Recognized non-sweep history features retain incomplete required operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.surface-operation-incomplete` | C | Surface construction history features retain incomplete required operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.sweep-incomplete` | C | Profile sweep history features retain incomplete required operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.brep-incomplete` | B | General model B-rep transfer remains incomplete for later instances. The omitted record payload has no typed document representation. |
| `geometry.instance-carriers-gated` | B | Remaining per-instance surfaces, curves, and vertices stay gated. The omitted record payload has no typed document representation. |
| `geometry.legacy-surface-carrier-unresolved` | B | A legacy ASCII surface carrier refused a lane, so its geometry stays a structural record. The omitted record payload has no typed document representation. |
| `geometry.nurbs-boundary-carrier-unresolved` | B | A NURBS boundary or plane-generator carrier refused a lane, so the curve named by its surface row stays a structural record. The omitted record payload has no typed document representation. |
| `geometry.plane-support-variants-capped` | B | Plane-support lane search stopped at its local variant or visit cap. The omitted record payload has no typed document representation. |
| `geometry.section-segment-unresolved` | C | Typed section segment records retain the source-native geometry. |
| `geometry.section-spline-unresolved` | A | A saved section spline cannot form a NURBS curve. The omitted record payload has no typed document representation. |
| `geometry.surface-prototype-field-retained` | B | A named surface-prototype field's bounded scalar body states no slots and is retained opaque. The omitted record payload has no typed document representation. |
| `geometry.triangle-strip-conflict` | C | Primitive triangle-strip records carry disagreeing position representations. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.visibgeom-curve-ambiguous` | C | VisibGeom curve-topology rows share a non-unique identity. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.visibgeom-curve-untransferred` | B | Unique VisibGeom curve-topology rows were not transferred as carriers. The omitted record payload has no typed document representation. |
| `geometry.visibgeom-surface-ambiguous` | C | VisibGeom surface rows share a non-unique identity. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.visibgeom-surface-untransferred` | B | Unique VisibGeom surface rows were not transferred as carriers. The omitted record payload has no typed document representation. |
| `legacy.byte-string-encoding-retained` | C | Legacy type-3 or type-4 byte-string retains non-UTF-8 source bytes. The source record survives as typed content; its neutral fidelity is reduced. |
| `legacy.compact-real-unresolved` | A | Legacy type-6 compact-real row did not form a complete finite scalar or array. The omitted record payload has no typed document representation. |
| `legacy.continuation-form-undefined` | A | Legacy type-3 or type-4 value row uses an undefined continuation form. The omitted record payload has no typed document representation. |
| `legacy.integer-value-unresolved` | A | Legacy type-1 integer row did not form a signed 32-bit scalar or array. The omitted record payload has no typed document representation. |
| `legacy.object-array-incomplete` | A | Legacy type-0 object array element count differs from declared extents. The omitted record payload has no typed document representation. |
| `legacy.object-payload-undefined` | A | Legacy type-0 value row uses an undefined object payload form. The omitted record payload has no typed document representation. |
| `legacy.real-value-unresolved` | A | Legacy type-2 real row did not form a complete finite scalar or array. The omitted record payload has no typed document representation. |
| `legacy.string-array-incomplete` | A | Legacy type-10 string array element count differs from its first extent. The omitted record payload has no typed document representation. |
| `legacy.string-continuation-undefined` | A | Legacy type-10 value row uses an undefined continuation form. The omitted record payload has no typed document representation. |
| `legacy.string-encoding-retained` | C | Legacy type-10 string element retains non-UTF-8 source bytes. The source record survives as typed content; its neutral fidelity is reduced. |
| `legacy.unsigned-value-unresolved` | A | Legacy type-5/7/9/11 row did not form an unsigned 32-bit scalar or array. The omitted record payload has no typed document representation. |
| `section.dimension-guess-unresolved` | C | Section solver pre-solve estimates use an unresolved dimension sentinel. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.dimension-value-unresolved` | C | Section dimensions retain source-native value tokens. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.dimension-variable-unresolved` | C | Dimension-driven section solver variables retain unresolved exact values. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.incidence-missing` | A | Declared section incidence rows did not decode. The omitted record payload has no typed document representation. |
| `section.incidence-native` | C | Active section incidence constraints retain native operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.relation-join-missing` | A | Declared section relation-incidence join rows did not decode. The omitted record payload has no typed document representation. |
| `section.relation-missing` | A | Declared section relation rows did not decode. The omitted record payload has no typed document representation. |
| `section.relation-native` | C | Active section dimension relations retain native operands. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.relation-table-malformed` | A | Section relation table uses the invalid zero allocation count. The omitted record payload has no typed document representation. |
| `section.segment-missing` | A | Declared section segment rows did not decode. The omitted record payload has no typed document representation. |
| `section.sign-branches-capped` | C | A section distance component had more sign branches than the local cap. The source record survives as typed content; its neutral fidelity is reduced. |
| `section.solver-variable-missing` | A | Declared section solver variable rows did not decode. The omitted record payload has no typed document representation. |
| `source.dialect-unverified` | D | No persistence-layout discriminant matched, so no layout-specific decode strategy was applied. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `topology.extrusion-body-rejected` | B | A resolved extrusion body failed shell admission. The omitted record payload has no typed document representation. |
| `topology.incomplete-components` | B | Remaining topology components lack complete face, curve, or vertex data. The omitted record payload has no typed document representation. |
| `topology.vertex-identifier-unstatable` | B | A half-edge orbit is past the stated topological vertex identifier width. The omitted record payload has no typed document representation. |

## F3D

| Local wire code | Class | Retention decision |
|---|---|---|
| `appearance.decal-native` | B | Native placement/mapping/name remains typed, but design/decode/{canvas,decal}.rs skips embedded_image_asset on unresolved neutral binding; raster pixels survive only as retained ZIP bytes; conservative when a sibling shares the asset |
| `appearance.image-asset-untransferred` | B | Design image entry absent from typed model.assets; pixels remain only retained ZIP bytes; verified by source AssetId scan before fidelity assembly |
| `appearance.reference-image-native` | B | Native placement/mapping/name remains typed, but design/decode/{canvas,decal}.rs skips embedded_image_asset on unresolved neutral binding; raster pixels survive only as retained ZIP bytes; conservative when a sibling shares the asset |
| `assembly.act-component-link-unresolved` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `assembly.component-insert-order-unknown` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `assembly.component-insert-unplaced` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `assembly.components-external` | C | typed external references and occurrence identities retained; externally referenced file payload not in this source |
| `assembly.joint-unprojected` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `assembly.local-occurrence-unplaced` | C | records/feature/assembly_features.rs retains typed occurrence identity, definition, parent and matrix; neutral placement is unresolved |
| `brep.header-metadata-unresolved` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `brep.header-tolerance-unresolved` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `brep.text-header-noncanonical` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `configuration.feature-suppression-unbound` | C | records/configuration.rs ConfigurationVariant.suppressed retains ordered names; neutral properties preserve suppression names |
| `configuration.member-unassigned` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `configuration.parameter-override-unbound` | C | records/configuration.rs ConfigurationScalar + design/configurations.rs native named scalar override retained; neutral properties preserve value |
| `configuration.rule-unbound` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `container.missing-geometry-stream` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `design.body-binding-unresolved` | C | records/bodies.rs DesignBodyBinding keeps blob_name, asm_body_key, entity_suffix and body relation; loss is unresolved join |
| `design.bodyless-carrier` | C | valid typed design-only document; informational route classification |
| `design.intent-undecoded` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `dimension.companion-untyped` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `dimension.unprojected` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `drawing.document-omitted` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `feature.body-selection-native` | C | typed native body recipe operands retained; missing raw AST independently adds design |
| `feature.definition-incomplete` | C | records/feature/scope.rs typed specialized operation payload retained; name-only/uninterpreted scopes independently add design |
| `feature.edge-operand-unresolved` | C | typed native edge recipe AST/identity retained; raw words independently add design |
| `feature.edge-reference-lost` | C | records/references.rs typed lost reference identity and next record index retained |
| `feature.edge-selection-lost` | C | typed lost-edge record retained; source-only recipe programs independently add design |
| `feature.edge-selection-native` | C | typed native edge recipe AST/identity retained; raw words independently add design |
| `feature.face-operand-unresolved` | C | typed native face recipe AST and typed identity candidates retained; raw words independently add design |
| `feature.face-selection-active-substituted` | C | typed native face selection retained and neutral active substitute explicit; no source record silently removed |
| `feature.face-selection-native` | C | typed native face operand/recipe AST retained; missing raw AST independently adds design |
| `feature.path-selection-native` | C | typed native path operands retained; missing raw AST independently adds design |
| `feature.pattern-instance-search-capped` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `feature.profile-arrangement-capped` | C | all typed profile/arrangement operands retained; cap affects derived neutral selection |
| `feature.profile-densification-capped` | C | typed native profile operand geometry retained; certification/densification bounded |
| `feature.profile-selection-native` | C | typed native scope/profile operands retained; missing raw AST independently adds design |
| `feature.recipe-search-budget-exceeded` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `feature.scope-payload-uninterpreted` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `feature.scope-unprojected` | A/C native-aware | C if specialized typed native scope payload retained; A if payload is Uninterpreted; Native(name)-only envelopes independently add A |
| `geometry.blend-support-partial` | B | ASM brep/emit.rs counts partial only when one resolved support and native construction absent; the second native support relationship survives only source bytes |
| `geometry.brep-blob-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.face-surface-dangling` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.mesh-surface-sentinel` | C | native typed mesh-surface sentinel semantics retained; missing mesh geometry charged separately |
| `geometry.not-transferred` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.nurbs-curve-carrier` | C | neutral typed solved NURBS curve retained |
| `geometry.nurbs-surface-carrier` | C | neutral typed solved NURBS surface retained |
| `geometry.pcurve-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.procedural-curve-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.solved-record-untyped` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `geometry.surface-shape-not-decoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `history.binding-budget-exceeded` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `history.binding-work-exceeded` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `history.dependency-ambiguous` | C | native feature scopes retain typed state reference candidates; ambiguity is in derived neutral dependency |
| `history.dependency-unprojected` | C | native feature scopes retain typed state references; opaque historical payload independently adds design unrepresented |
| `history.face-selection-unbound` | C | records/topology/historical_context.rs retains typed face-context identities; raw recipe or history payload independently adds design omission |
| `history.record-framing-failed` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `material.distance-unit-untyped` | B | decode.rs report_untyped_material_distances explicitly omits typed texture carriers; materials.rs keeps only count for unknown-unit texture Distance property |
| `material.not-transferred` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `mesh.attribute-not-transferred` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `mesh.container-missing` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `mesh.container-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `mesh.container-unjoined` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `mesh.vertex-precision-reduced` | C | neutral typed tessellation holds quantized vertices; only numeric precision reduced |
| `parameter.expression-unbound` | C | records/parameters.rs literal symbolic expression retained; only derived dependency edge unresolved |
| `parameter.order-budget-exceeded` | C | design/feature_project.rs retains all scalar/expression records and source ordinals; only derived dependency ordering is bounded |
| `parameter.owner-unrecognized` | A | source design payload absent from every typed arena; source-only framing/payload/unknown/native envelope cannot supply content |
| `parameter.unit-untyped` | C | records/parameters.rs typed unit token and evaluated scalar retained; canonical quantity kind unresolved |
| `parameter.unprojected` | C | records/parameters.rs native DesignParameter retains name, source family, expression, unit and evaluated scalar |
| `sketch.curve-unprojected` | A/C native-aware | C when SketchCurveIdentity.geometry is typed; A when geometry is None |
| `sketch.dimension-native` | C | typed dimension parameter and native operand identity retained; companion payload-only omissions separately add design |
| `sketch.placement-unprojected` | C | records/sketch_placement.rs typed transform/placement frames remain native |
| `sketch.point-unprojected` | C | records/sketch_geometry.rs typed point coordinates and depth remain native |
| `sketch.profile-search-capped` | C | all sketch entities/curves retained; bounded operation derives neutral closed-profile grouping only |
| `sketch.relation-native` | C | records/sketch_relations.rs typed relation members, operands, return members and state preserved |
| `sketch.relation-unprojected` | C | records/sketch_relations.rs typed relation definition and operands retained |
| `sketch.surface-unprojected` | C | records/sketch_geometry.rs full typed NURBS surface retained natively |
| `sketch.text-unprojected` | C | records/sketch_geometry.rs typed text, font, height, color, layout and owning identities retained; raw bytes are not a representation fallback |
| `source.archive-entry-unreadable` | A+B | unreadable archive entry role is unresolved; conservatively both model and design |
| `source.dialect-layer-collision` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `source.dialect-unverified` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `source.kernel-carrier-unparseable` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `source.kernel-dialect-unverified` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `source.manifest-metadata-unresolved` | D | source identity/header/descriptive metadata classification, independent of arena content |
| `source.preserved-image-unavailable` | (w) | writer-side only; never contributes decode fidelity omissions |
| `target.source-dialect-displaced` | (w) | writer-side only; never contributes decode fidelity omissions |
| `topology.not-transferred` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `tspline.cage-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `tspline.record-untyped` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `writer.appearance-properties-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.asset-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.auxiliary-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.design-records-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.geometry-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.native-records-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.no-exportable-solids` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.pcurve-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.pmi-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.procedural-reduced` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.product-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.source-association-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.subd-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `writer.tessellation-omitted` | (w) | writer-side only; never contributes decode fidelity omissions |
| `xref.cycle` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `xref.member-missing` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `xref.member-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `xref.placement-superseded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `xref.placement-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |
| `xref.table-undecoded` | B | source geometry/topology/product/material payload absent from every typed arena; issue/envelope/unknown records do not retain the payload |

## FreeCAD

| Local wire code | Class | Retention decision |
|---|---|---|
| `appearance.not-transferred` | (w) | Unsupported appearance members are omitted with a Warning loss. Export-only code; does not change decode fidelity. |
| `appearance.primitive-size-not-transferred` | B | Model appearance property was not delivered. |
| `appearance.topology-color-count-mismatch` | B | Topology appearance content was not delivered. |
| `arena.not-transferred` | (w) | Export-only; does not change decode fidelity. |
| `element-map.metadata-unresolved` | B | Unparsed mapped-name metadata remains source only. |
| `element-map.version-unresolved` | B | Map remains source XML, with no admitted typed map. |
| `feature.cyclic-history` | C | Typed native object/property graph retains the cycle. |
| `feature.native-kind-retained` | C | Typed native object/property graph retains operation. |
| `feature.not-transferred` | (w) | Export-only; does not change decode fidelity. |
| `feature.object-unprojected` | C; A if an owned property is Unreadable | Native object and fully typed properties retain excluded construction state. Loss is Blocking, FeatureHistoryRetained. |
| `geometry.not-transferred` | (w) | Export-only; does not change decode fidelity. |
| `geometry.procedural-reduced` | (w) | A procedural carrier is written through its current solved cache with a Warning loss. Export-only code; does not change decode fidelity. |
| `metadata.file-version-noncanonical` | D | Metadata spelling only. |
| `metadata.file-version-unverified` | D | Version declaration only. |
| `metadata.native-property-not-transferred` | (w) | Export-only; does not change decode fidelity. |
| `metadata.persistence-count-noncanonical` | D | Framed admitted rows remain authoritative. |
| `metadata.persistence-payload-unresolved` | A/B by owner/type | Unreadable on design object marks design. Part::PropertyPartShape marks model. Document parameters mark design; other model object properties mark model. |
| `metadata.persistence-side-entry-unresolved` | A/B by owner/type | A typed filename does not represent the missing payload. |
| `metadata.program-version-noncanonical` | D | Metadata spelling only. |
| `pcurve.not-transferred` | B | Required parameter-space model content was not delivered. |
| `presentation.not-transferred` | (w) | GUI carrier requirements or aliases alter neutral presentation state; the writer reports a Warning loss. Export-only code; does not change decode fidelity. |
| `sketch.external-cache-unresolved` | A | Cache did not project and survives only in source payload/XML. |
| `sketch.external-reference-unresolved` | C | Typed native reference property retains link content. |
| `sketch.native-constraint` | C | Typed native constraint property retains relation content. |
| `sketch.native-geometry` | C | Typed native geometry properties retain carrier content. |
| `sketch.profile-matching-bounded` | C | Native sketch records and neutral geometry remain; profile chains are derived. |
| `source.archive-entry-unreadable` | A/B by served class | unreadable_entries retains stored bytes and diagnostics only. Native property references and GUI/shape/thumbnail names select class; unknown entries conservatively mark both. |
| `source.design-payload-unrepresented` | A | A design-owned side-entry or inline Python payload survives only as stored bytes, a filename, or opaque property data. |
| `source.dialect-unverified` | D | Dialect declaration metadata only. |
| `source.gui-metadata-unresolved` | B | GUI XML remains source only. |
| `source.gui-property-key-blank` | B | Unnamed model presentation property was not projected. |
| `source.gui-schema-unverified` | D | Source spelling differs; GUI grammar projection still runs. |
| `source.model-payload-unrepresented` | B | A model-owned side-entry or inline Python payload survives only as stored bytes, a filename, or opaque property data. Unowned side entries conservatively also declare design. |
| `writer.brep-not-regenerated` | (w) | Export-only; does not change decode fidelity. |

FreeCAD Geometry decoding retains fully typed scalar, vector, link, and placement properties even when their neutral design projection is omitted. Those native records remain represented. A typed filename plus stored entry bytes does not retain a binary design payload: Geometry decoding declares design unrepresented for such entries. Full decoding represents known Nodes/Spacings/Spacings2/SpacingPattern/SpacingPattern2 lists only when the corresponding non-native neutral feature was delivered, and sketch geometry/constraint entries only when the neutral sketch was delivered without an unresolved native geometry or constraint loss. Typed shape payloads, admitted string tables and element maps, typed product-node placement/scale lists, delivered mesh/point carriers, and delivered GUI appearance lists are represented independently of neutral design scope. JSON-mode Python values are represented through their editable decoded JSON field; the writer regenerates its encoded form from that field. Opaque File/PythonObject side entries and non-JSON pickle/text payloads remain unrepresented at either request.

## IGES

| Local wire code | Class | Retention decision |
|---|---|---|
| `card.framing-recovered` | D | Card framing came from the card census because the file's own declaration did not. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `curve.composite-carrier-degraded` | C | A Type 102 composite has no admitted concatenated carrier. The source record survives as typed content; its neutral fidelity is reduced. |
| `design.parametric-records-omitted` | (w) | Parametric design records have no IGES encoding. Export-only code; does not change decode fidelity. |
| `directory.metadata-noncanonical` | C | A readable display field violates a source rule without changing geometry. The source record survives as typed content; its neutral fidelity is reduced. |
| `directory.metadata-unreadable` | B | A Directory presentation or naming field remains source-only. The omitted record payload has no typed document representation. |
| `directory.record-quarantined` | B | One Directory Entry record kept its raw cards because its typed fields were not recovered. The omitted record payload has no typed document representation. |
| `entity.not-projected` | B | An entity was not projected; the instance message names the reason. The omitted record payload has no typed document representation. |
| `entity.outside-envelope` | B | An entity type/form is outside the Fixed ASCII mechanical/document envelope. The omitted record payload has no typed document representation. |
| `entity.retained-unprojected` | B | An envelope-admitted entity was retained without a neutral projection. The omitted record payload has no typed document representation. |
| `export.no-exportable-solids` | (w) | No writable geometric entity remains in the document. Export-only code; does not change decode fidelity. |
| `geometry.not-projected` | B | A recognized geometric entity could not retain its required geometry. The omitted record payload has no typed document representation. |
| `geometry.nurbs-transform-non-finite` | B | A NURBS coordinate or parameter transformation produced a non-finite value. The omitted record payload has no typed document representation. |
| `geometry.procedural-reduced` | (w) | Procedural definitions were reduced to writable solved carriers. Export-only code; does not change decode fidelity. |
| `geometry.ruled-developability-not-transferred` | C | A Type 118 developability flag was not transferred to neutral geometry. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.ruled-developability-recovered` | C | An unusable Type 118 developability assertion did not change its rails. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.spline-claim-recovered` | C | Redundant spline claims disagreed with the serialized carrier mathematics. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.spline-header-not-transferred` | C | Type 112 or Type 114 header semantics were not transferred to neutral geometry. The source record survives as typed content; its neutral fidelity is reduced. |
| `global.length-unit-unresolved` | D | The Global fields 13, 14, and 15 produced no millimetre length factor. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `global.metadata-field-unusable` | D | A Global metadata field is absent or malformed and was not transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `global.noncanonical-framing` | D | Global framing is recoverable but noncanonical for the declared profile. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `global.numeric-syntax-recovered` | D | A Global real used a recoverable noncanonical numeric spelling. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `global.semantic-context-substituted` | D | A Global comparison context came from this codec's specification. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `graph.pointer-unresolved` | B | A Directory Entry pointer did not resolve to the expected target. The omitted record payload has no typed document representation. |
| `occurrence.expansion-depth-truncated` | B | Product-occurrence expansion stopped at the configured nesting-depth limit. The omitted record payload has no typed document representation. |
| `occurrence.expansion-output-truncated` | B | Product-occurrence expansion stopped at the configured output limit. The omitted record payload has no typed document representation. |
| `occurrence.placement-malformed` | B | Product-occurrence expansion omitted an instance or member with malformed placement data. The omitted record payload has no typed document representation. |
| `occurrence.root-inference-blocked` | C | Product-occurrence root inference was suppressed by a malformed member list. The source record survives as typed content; its neutral fidelity is reduced. |
| `parameter.attribute-table-count-unstatable` | B | A Type 422 attribute table states a count no row grid can be read from. The omitted record payload has no typed document representation. |
| `parameter.boundary-ambiguous` | C | Parameter Data has more than one structural trailing pointer-group boundary. The source record survives as typed content; its neutral fidelity is reduced. |
| `parameter.count-overdeclared` | C | A counted list declares more items than its Parameter Data record holds. The source record survives as typed content; its neutral fidelity is reduced. |
| `parameter.data-quarantined` | B | One entity's Parameter Data kept its raw cards because its tokens were not recovered. The omitted record payload has no typed document representation. |
| `parameter.literal-unusable` | B | A framed Parameter Data literal is retained without an interpreted value. The omitted record payload has no typed document representation. |
| `presentation.body-name-ambiguous` | C | A body owner has conflicting valid Type 406 Form 15 names. The source record survives as typed content; its neutral fidelity is reduced. |
| `presentation.display-data-not-projected` | B | Directory display, font, or color data was not projected. The omitted record payload has no typed document representation. |
| `presentation.drawing-property-ambiguous` | C | A drawing has conflicting valid properties of the same form. The source record survives as typed content; its neutral fidelity is reduced. |
| `presentation.line-weight-scale-unavailable` | B | The Global line-weight scale is unavailable, so no entity has a width. The omitted record payload has no typed document representation. |
| `source.dialect-unverified` | D | The declared Global specification version is outside the verified set. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.preserved-image-unavailable` | (w) | Preserved source image required for a byte-exact write was unavailable. Export-only code; does not change decode fidelity. |
| `target.source-dialect-displaced` | (w) | The selected write target differs from the same-format source dialect. Export-only code; does not change decode fidelity. |
| `topology.boundary-pcurve-outside-support-domain` | C | A boundary pcurve leaves the finite parameter domain of its support surface. The source record survives as typed content; its neutral fidelity is reduced. |
| `writer.arena-omitted` | (w) | A neutral arena has no IGES encoding and was omitted. Export-only code; does not change decode fidelity. |
| `writer.body-color-not-represented` | (w) | A body has no owning Directory Entry for its color. Export-only code; does not change decode fidelity. |
| `writer.body-name-not-represented` | (w) | A body name cannot be encoded as a Type 406 Form 15 name. Export-only code; does not change decode fidelity. |
| `writer.body-opacity-not-represented` | (w) | IGES Directory color cannot carry a body opacity value. Export-only code; does not change decode fidelity. |
| `writer.body-transform-not-applied` | (w) | A body's local shape was written without its unsupported placement. Export-only code; does not change decode fidelity. |
| `writer.body-visibility-not-represented` | (w) | A free-geometry body has no single Directory Entry for visibility. Export-only code; does not change decode fidelity. |
| `writer.duplicate-carrier-omitted` | (w) | Free edges or curves that duplicate a written carrier were not written. Export-only code; does not change decode fidelity. |
| `writer.entity-omitted` | (w) | A geometric entity or containing body has no valid IGES encoding. Export-only code; does not change decode fidelity. |
| `writer.free-geometry-omitted` | (w) | Free-geometry points or vertices have no topology output. Export-only code; does not change decode fidelity. |
| `writer.minimum-resolution-adjusted` | (w) | The emitted Global minimum resolution exceeds the neutral declaration. Export-only code; does not change decode fidelity. |
| `writer.passthrough-omitted` | (w) | A native passthrough arena is not regenerated by the semantic writer. Export-only code; does not change decode fidelity. |
| `writer.pcurves-omitted` | (w) | Unsupported UV images were omitted while preserving model-space boundaries. Export-only code; does not change decode fidelity. |
| `writer.support-edge-not-represented` | (w) | An unowned dependent edge was withheld. Export-only code; does not change decode fidelity. |
| `writer.support-geometry-not-represented` | (w) | Unowned support geometry was withheld to avoid standalone transfer. Export-only code; does not change decode fidelity. |

## Inventor

| Local wire code | Class | Retention decision |
|---|---|---|
| `appearance.default-unresolved` | B | PmApp document-default appearance assignments did not resolve. The omitted record payload has no typed document representation. |
| `appearance.face-override-unresolved` | B | PmGraphics face appearance overrides did not resolve. The omitted record payload has no typed document representation. |
| `assembly.component-external` | B | External component references remain unresolved. The omitted record payload has no typed document representation. |
| `assembly.placement-not-transferred` | B | Assembly occurrence placements could not be transferred. The omitted record payload has no typed document representation. |
| `assembly.record-malformed` | B | Typed assembly records are malformed or outside the implemented branch. The omitted record payload has no typed document representation. |
| `design.record-malformed` | A | Typed design records are malformed or outside the implemented branch. The omitted record payload has no typed document representation. |
| `feature.label-unusable` | C | An optional feature label is empty and has no neutral display name. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.operation-graph-open` | A | Typed feature records have an operation graph that is not closed. The omitted record payload has no typed document representation. |
| `feature.record-malformed` | A | Typed feature records could not be parsed exactly. The omitted record payload has no typed document representation. |
| `feature.state-unresolved` | C | Inventor operations retain native result-body identity with unresolved state. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.kernel-carrier-not-transferred` | B | The active kernel carrier was not transferred into neutral geometry. The omitted record payload has no typed document representation. |
| `geometry.procedural-surface-not-transferred` | B | Faces use procedural surfaces without a decoded carrier. The omitted record payload has no typed document representation. |
| `material.distance-unit-untyped` | C | A Protein texture states a distance with no length conversion. The source record survives as typed content; its neutral fidelity is reduced. |
| `metadata.property-unmapped` | D | Property values have no neutral metadata mapping. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parameter.expression-bounded` | A/C | Actual values retained in typed native.inventor.pm_dc_expressions; if absent, class A. |
| `parameter.graph-open` | A | Parameter records have a unit or expression graph that is not closed. The omitted record payload has no typed document representation. |
| `presentation.record-malformed` | B | Typed presentation records are malformed or outside the implemented branch. The omitted record payload has no typed document representation. |
| `property-set.stream-malformed` | D | OLE property-set streams are malformed. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `protein.appearance-absent` | B | The Protein package contains no decoded appearance assets. The omitted record payload has no typed document representation. |
| `protein.asset-rejected` | B | Malformed Protein asset records were rejected. The omitted record payload has no typed document representation. |
| `protein.catalog-undecodable` | B | The Protein asset catalog could not be decoded. The omitted record payload has no typed document representation. |
| `protein.guid-ambiguous` | C | Protein catalog asset GUIDs collide; ambiguous texture joins were refused. The source record survives as typed content; its neutral fidelity is reduced. |
| `protein.stream-malformed` | B | The Inventor Protein stream is malformed. The omitted record payload has no typed document representation. |
| `rse.bulk-stream-malformed` | A+B | RSe bulk streams have invalid envelope or zlib framing. The omitted record payload has no typed document representation. |
| `rse.metadata-stream-malformed` | A+B | RSe metadata streams are malformed or outside the implemented envelope. The omitted record payload has no typed document representation. |
| `rse.segment-pair-untyped` | A+B | Structurally paired RSe segments have no record semantics. The omitted record payload has no typed document representation. |
| `rse.stream-unpaired` | A+B | RSe contains unpaired metadata or bulk streams. The omitted record payload has no typed document representation. |
| `sketch.graph-open` | A | Sketch, entity, or constraint records have a neutral graph that is not closed. The omitted record payload has no typed document representation. |
| `sketch.record-malformed` | A | Typed sketch records could not be parsed exactly. The omitted record payload has no typed document representation. |
| `source.dialect-unverified` | D | The document was read with a grammar its own declarations do not select. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.kernel-carrier-unparseable` | B | The selected kernel carrier did not expose a parseable kernel header. The omitted record payload has no typed document representation. |
| `source.kernel-dialect-unverified` | D | The active kernel carrier used an unverified Spatial ACIS grammar band. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.kernel-header-metadata-unresolved` | D | An independently framed kernel product string is unreadable. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.kernel-header-tolerance-unresolved` | D | An optional kernel tolerance cannot enter document units. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `ufrx.schema-unsupported` | B | An unsupported UFRxDoc schema was retained without a typed transfer. The omitted record payload has no typed document representation. |
| `ufrx.schema-unsupported-assembly` | B | An unsupported UFRxDoc schema was retained on an assembly document. The omitted record payload has no typed document representation. |
| `ufrx.table-malformed` | B | The UFRxDoc external-reference table is malformed. The omitted record payload has no typed document representation. |

## NX

| Local wire code | Class | Retention decision |
|---|---|---|
| `annotation.semantic-order-unstatable` | B | A semantic annotation order is past the stated annotation order width. The omitted record payload has no typed document representation. |
| `assembly.components-external` | B | Assembly .prt has no inline geometry; children live in external parts. The omitted record payload has no typed document representation. |
| `attribute.value-unresolved` | B | A referenced Parasolid attribute value relation did not resolve. The omitted record payload has no typed document representation. |
| `carrier.analytic-census` | D | Census of decoded Parasolid POINT and analytic curve/surface carriers. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `carrier.display-jt-tessellations-dropped` | B | A Display-JT polygon mesh did not resolve to a tessellation and was skipped. The omitted record payload has no typed document representation. |
| `carrier.lanes-unpaired` | B | A carrier record of the right family states lanes the IR carrier refuses. The omitted record payload has no typed document representation. |
| `carrier.tessellation-census` | D | Census of decoded embedded JT display tessellations. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `configuration.state-unresolved` | C | Configuration activation, body membership, or evaluated state is incomplete. The source record survives as typed content; its neutral fidelity is reduced. |
| `container.display-jt-arithmetic-lane-bounded` | B | An arithmetic-coded Display-JT lane reached its decoder work bound and was left undecoded. The omitted record payload has no typed document representation. |
| `container.display-jt-graph-rejected` | B | A JT display graph failed native admission. The omitted record payload has no typed document representation. |
| `container.offset-store-control-untyped` | A+B | Bounded offset-store control blocks have no admitted complete grammar. The omitted record payload has no typed document representation. |
| `container.stream-opaque` | A+B | A named container stream is retained byte-exact without typed fields. The omitted record payload has no typed document representation. |
| `deltas.applied` | D | Parasolid deltas applied; every terminal tombstone resolved to a key. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `deltas.unmatched-tombstones` | D | Parasolid deltas applied; one or more terminal tombstones remain unmatched. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `expression.parameter-incomplete` | C | Expression parameter evaluation or dependency semantics are incomplete. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.construction-incomplete` | C | Typed feature operations have incomplete neutral construction fields. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.family-construction-unresolved` | C | Feature family identities transferred; construction semantics unresolved. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.native-kind-retained` | A | Feature-history operations remain native-only without neutral semantics. The omitted record payload has no typed document representation. |
| `feature.output-lineage-incomplete` | C | Typed feature output lineage is missing, duplicated, or unresolved. The source record survives as typed content; its neutral fidelity is reduced. |
| `feature.suppression-unresolved` | C | Feature-history suppression state remains unresolved. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.adaptive-work-bounded` | B | Adaptive geometry certification reached its model-wide work bound. The omitted record payload has no typed document representation. |
| `geometry.not-transferred` | B | No gate-passing analytic carrier was found in the Parasolid streams. The omitted record payload has no typed document representation. |
| `history.roll-forward-table-rejected` | A | A roll-forward table failed native admission. The omitted record payload has no typed document representation. |
| `history.sub-body-composition-unresolved` | C | Sub-body partitions remain; Boolean history does not resolve every image. The source record survives as typed content; its neutral fidelity is reduced. |
| `intersection.pcurve-completion-bounded` | B | Geometric completion reached a declared work bound before all intersection pcurve lanes were complete. The omitted record payload has no typed document representation. |
| `intersection.records-opaque` | B | Surface-intersection records lack a validated chart and term-endpoint witness. The omitted record payload has no typed document representation. |
| `native.attachment-failed` | A+B | Native object-model attachment failed; none of its records were kept. The omitted record payload has no typed document representation. |
| `native.record-identity-bounded` | A+B | Indexed OM object records reached the identity graph work bound and were kept without a stable identity. The omitted record payload has no typed document representation. |
| `sketch.graph-unresolved` | A | Sketch history features have no neutral sketch graph. The omitted record payload has no typed document representation. |
| `sketch.native-semantics` | C | Sketch geometry or constraint records remain native-only. The source record survives as typed content; its neutral fidelity is reduced. |
| `source.dialect-layer-collision` | D | Two embedded kernel carriers resolved to one dialect-layer identity. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.kernel-dialect-unverified` | D | An embedded kernel dialect had no declared grammar and was recovered as residual. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.scan-bounded` | A+B | A record search reached its local work bound and skipped the records it was locating. The omitted record payload has no typed document representation. |
| `stream.non-parasolid-omitted` | B | A classified non-Parasolid stream was not transferred. The omitted record payload has no typed document representation. |
| `topology.face-loop-unresolved` | B | A face's linked loop boundary cannot be admitted and is omitted. The omitted record payload has no typed document representation. |
| `topology.graph-not-reconstructed` | B | B-rep topology graph was not reconstructed from surviving typed records. The omitted record payload has no typed document representation. |
| `topology.loop-ring-unresolved` | B | A face loop states no resolvable coedge ring and is omitted from its face. The omitted record payload has no typed document representation. |

## Rhino

| Local wire code | Class | Retention decision |
|---|---|---|
| `annotation.record-dropped` | B | A framed annotation record could not be transferred. The omitted record payload has no typed document representation. |
| `annotation.userdata-dropped` | B | A recognized annotation userdata payload could not be typed. The omitted record payload has no typed document representation. |
| `appearance.color-precision-reduced` | (w) | Object display colors were quantized to native byte channels. Export-only code; does not change decode fidelity. |
| `body.non-rigid-transform` | (w) | A body placement cannot be represented in the native carrier. Export-only code; does not change decode fidelity. |
| `brep.mesh-cache-degraded` | B | A Brep display-mesh cache slot was wrong-class or unreadable. The omitted record payload has no typed document representation. |
| `brep.trim-pcurve-dropped` | B | One B-rep trim lost its parameter-space curve while its topology remained. The omitted record payload has no typed document representation. |
| `cage.lattice-not-transferred` | B | Trivariate cage control lattice is retained as text, not a typed deformation. The omitted record payload has no typed document representation. |
| `container.duplicate-record-resolved` | C | A duplicate source record was resolved by the format's ownership rule. The source record survives as typed content; its neutral fidelity is reduced. |
| `container.enumeration-value-degraded` | C | A stored enumeration value was retained but could not select a neutral value. The source record survives as typed content; its neutral fidelity is reduced. |
| `container.instance-definition-degraded` | B | Instance-definition records were malformed, ambiguous, or checksum-degraded. The omitted record payload has no typed document representation. |
| `container.integrity-failure` | D | A stored checksum does not match the protected bytes. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `container.redundant-field-repaired` | C | A redundant count or size was inconsistent; dependent data was dropped. The source record survives as typed content; its neutral fidelity is reduced. |
| `container.scan-diagnostic` | D | Container or table scan surfaced a structural diagnostic. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `curve-on-surface.binding-not-transferred` | C | Native feature parameters retain the curve/surface binding; neutral carriers retain the decoded geometry. |
| `curve.polycurve-join-gap` | C | A discontinuous polycurve join moved both source endpoints to their midpoint. The source record survives as typed content; its neutral fidelity is reduced. |
| `design.parametric-records-omitted` | (w) | A document arena has no 3DM representation. Export-only code; does not change decode fidelity. |
| `detail.view-not-transferred` | B | Detail-view projection state is retained as a digest, not a decoded view. The omitted record payload has no typed document representation. |
| `dimension.detail-reference-unresolved` | B | A dimension detail-view reference does not resolve to a decoded view. The omitted record payload has no typed document representation. |
| `dimension.override-dropped` | B | A dimension-style override object was not applied. The omitted record payload has no typed document representation. |
| `dimension.style-unresolved` | B | A dimension style reference does not resolve to a decoded style record. The omitted record payload has no typed document representation. |
| `export.no-exportable-solids` | (w) | No geometry can be written to the target. Export-only code; does not change decode fidelity. |
| `hatch.fill-not-transferred` | C | Native FeatureOperation parameters retain hatch settings; neutral curves retain all hatch loops. |
| `history.dependency-dropped` | A | A history dependency points to a later producer and cannot enter ordered IR. The omitted record payload has no typed document representation. |
| `history.embedded-geometry-dropped` | B | Embedded history geometry could not be decoded. The omitted record payload has no typed document representation. |
| `history.geometry-not-transferred` | B | History-record geometry is retained without a neutral carrier. The omitted record payload has no typed document representation. |
| `mesh.ngon-grouping-dropped` | B | Mesh n-gon grouping is not represented in neutral tessellation. The omitted record payload has no typed document representation. |
| `mesh.normal-precision-reduced` | (w) | Mesh normals are written at reduced (f32) precision. Export-only code; does not change decode fidelity. |
| `mesh.quad-topology-triangulated` | C | A stored quad was converted to neutral triangles. The source record survives as typed content; its neutral fidelity is reduced. |
| `mesh.vertex-precision-reduced` | (w) | Standalone mesh vertices are written at reduced (f32) precision. Export-only code; does not change decode fidelity. |
| `morph.deformation-not-applied` | C | Native feature parameters retain the decoded morph and captive references; evaluation is not applied. |
| `object.attributes-degraded` | B | Object attributes (name, layer, color, visibility) did not transfer whole. The omitted record payload has no typed document representation. |
| `object.decode-diagnostic` | B | A decode phase surfaced a per-record diagnostic. The omitted record payload has no typed document representation. |
| `object.family-not-transferred` | B | An object class is not decoded; its records carry only retained bytes. The omitted record payload has no typed document representation. |
| `object.framing-undecodable` | B | A framed object record could not be decoded from its payload. The omitted record payload has no typed document representation. |
| `object.record-census` | D | Census of object records framed against object records transferred. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `polyedge.references-not-resolved` | C | Native feature parameters retain polyedge segment references and intervals. |
| `presentation.record-dropped` | B | A framed presentation record could not be transferred. The omitted record payload has no typed document representation. |
| `product.occurrence-dropped` | B | A framed product occurrence could not be transferred to the native graph. The omitted record payload has no typed document representation. |
| `reference.member-ambiguous` | B | A definition member or captive object UUID resolves to several records. The omitted record payload has no typed document representation. |
| `reference.member-unresolved` | B | A definition member or captive object UUID does not resolve to one record. The omitted record payload has no typed document representation. |
| `source.dialect-unverified` | D | The archive-version word is outside the declared set, so the document was read by the residual chunk-width route without a declared identity.  Document level, and the counterpart of the per-record code above: charged exactly when the primary-layer crate::dialect match is Admission::Residual, from the same predicate that decides the admission. A word no row claims still selects its own chunk width and checksum mechanics. No declared archive row is substituted, and nothing verified that the observed word means those mechanics. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.writer-stamp-unverified` | D | A field was read under the legacy reading because the archive carries no openNURBS writer-version stamp to verify that record against.  Per record, inside a dialect the archive word already identified and whose grammar this codec verified. It is deliberately *not* source.dialect-unverified: the other codecs pin that string for the document-level statement "the dialect itself was not verified", which holds exactly when Admission::Residual is reported. Rhino charges this one inside Admission::Admitted documents, so a consumer joining loss code to admission state must be able to tell them apart. The taxonomy family below is still the right one. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `target.source-dialect-displaced` | (w) | The selected write target differs from the same-format source dialect. Export-only code; does not change decode fidelity. |
| `topology.body-kind-gauge-substituted` | C | Body kind came from the closed-shell gauge or from an unverified stored solid flag rather than from a flag the writer stamp vouches for. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.brep-fallback` | B | B-rep topology fell back to a carrier-only transfer. The omitted record payload has no typed document representation. |
| `viewport.userdata-dropped` | B | Viewport userdata has no typed CADIR owner. The omitted record payload has no typed document representation. |
| `writer.attributes-omitted` | (w) | Attribute records have no writer encoding. Export-only code; does not change decode fidelity. |
| `writer.body-omitted` | (w) | A body cannot be encoded without its unsupported topology. Export-only code; does not change decode fidelity. |
| `writer.entity-omitted` | (w) | An entity has no writable carrier. Export-only code; does not change decode fidelity. |
| `writer.materials-omitted` | (w) | Material records have no writer encoding. Export-only code; does not change decode fidelity. |
| `writer.procedural-reduced` | (w) | Procedural definitions are reduced to solved carriers. Export-only code; does not change decode fidelity. |
| `writer.subds-omitted` | (w) | Subdivision records have no writer encoding. Export-only code; does not change decode fidelity. |
| `writer.support-geometry-withheld` | (w) | Standalone support points and curves cannot keep their dependency. Export-only code; does not change decode fidelity. |

## SAT

| Local wire code | Class | Retention decision |
|---|---|---|
| `edge.tolerance-unresolved` | C | Tolerant-edge tolerance cannot enter the positive neutral scalar. The source record survives as typed content; its neutral fidelity is reduced. |
| `framing.records-unread` | B | Bytes after the last framed record were not read as records. The omitted record payload has no typed document representation. |
| `framing.terminator-missing` | D | The input ends without a terminator line after its last complete record. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.framed-without-carriers` | B | The stream framed but decoded no surfaces, points, or faces. The omitted record payload has no typed document representation. |
| `geometry.procedural-surface-untyped` | B | A face rests on a procedural surface construction without a decoded carrier. The omitted record payload has no typed document representation. |
| `geometry.use-curve-interval-invalid` | B | A tolerant-coedge use curve has decreasing carrier endpoints. The omitted record payload has no typed document representation. |
| `header.length-unit-unresolved` | D | The stream declares no usable length unit; lengths are read unscaled as millimetres. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `header.metadata-noncanonical` | D | Independently framed product metadata is malformed. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `header.tolerance-unresolved` | D | A header tolerance cannot supply a positive finite document tolerance. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.concatenated-streams-recovered` | D | Independently headed text streams have separate local reference tables. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.kernel-dialect-unverified` | D | The stream was read with a grammar its own save-format declaration does not select. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `source.record-extensions-unprojected` | B | ACIS record extensions are retained without neutral projection. The omitted record payload has no typed document representation. |
| `source.record-name-unresolved` | B | A framed record has no leading source-name component. The omitted record payload has no typed document representation. |
| `source.stream-layout-unprojected` | B | A later text stream has an unreadable or different native record layout. The omitted record payload has no typed document representation. |
| `topology.face-owner-unprojected` | B | A standalone source face has no shell owner required by the IR. The omitted record payload has no typed document representation. |
| `topology.shell-unprojected` | B | A shell has no admissible member or owner. The omitted record payload has no typed document representation. |
| `topology.wire-owner-unprojected` | B | A body-owned source wire cannot use the IR's shell ownership. The omitted record payload has no typed document representation. |
| `vertex.tolerance-unresolved` | C | A tolerant vertex has no positive finite evaluated tolerance. The source record survives as typed content; its neutral fidelity is reduced. |

## SLDPRT

| Local wire code | Class | Retention decision |
|---|---|---|
| `appearance.assignment-unresolved` | B | Records lack a typed model representation. |
| `appearance.face-color-unresolved` | B | Records lack a typed model representation. |
| `config.active-identity-unresolved` | C | Typed neutral or native records retain the reduced content. |
| `config.active-partition-mismatch` | C | Typed neutral or native records retain the reduced content. |
| `config.ambiguous-naming` | C | Typed neutral or native records retain the reduced content. |
| `config.ambiguous-partition` | C | Typed neutral or native records retain the reduced content. |
| `config.incoherent-body-refs` | C | Typed neutral or native records retain the reduced content. |
| `config.incomplete-snapshot` | C | Typed neutral or native records retain the reduced content. |
| `config.inferred-without-native` | C | Typed neutral or native records retain the reduced content. |
| `config.lane-identity-unresolved` | C | Typed neutral or native records retain the reduced content. |
| `config.projection-work-capped` | C | Typed neutral or native records retain the reduced content. |
| `container.no-parasolid-stream` | B | Records lack a typed model representation. |
| `container.parasolid-probe-capped` | B | Records lack a typed model representation. |
| `export.no-exportable-solids` | (w) | Export-only; does not affect decode fidelity. |
| `feature.body-retention-unresolved` | C | Typed neutral or native records retain the reduced content. |
| `feature.incoherent-content` | C | Typed neutral or native records retain the reduced content. |
| `feature.incoherent-edges` | C | Typed neutral or native records retain the reduced content. |
| `feature.incoherent-outputs` | C | Typed neutral or native records retain the reduced content. |
| `feature.input-object-unbound` | A | Class/name framing and raw payload do not retain semantic operands. |
| `feature.native-kind-retained` | C | Typed neutral or native records retain the reduced content. |
| `feature.typed-operand-incomplete` | C | Typed neutral or native records retain the reduced content. |
| `feature.unresolved-output-scope` | C | Typed neutral or native records retain the reduced content. |
| `geometry.edge-support-curve-untyped` | B | Records lack a typed model representation. |
| `geometry.face-support-surface-untyped` | B | Records lack a typed model representation. |
| `geometry.parasolid-not-transferred` | B | Records lack a typed model representation. |
| `geometry.pcurve-ambiguous` | B | Records lack a typed model representation. |
| `geometry.spline-lanes-unpaired` | B | Records lack a typed model representation. |
| `geometry.surface-solver-work-capped` | B | Records lack a typed model representation. |
| `history.incomplete-references` | C | Typed neutral or native records retain the reduced content. |
| `material.metadata-not-transferred` | B | Records lack a typed model representation. |
| `parameter.ambiguous-identity` | C | Typed neutral or native records retain the reduced content. |
| `parameter.unevaluated` | C | Typed neutral or native records retain the reduced content. |
| `pmi.dimension-unbound` | C | Typed neutral or native records retain the reduced content. |
| `pmi.semantic-parse-capped` | B | Records lack a typed model representation. |
| `pmi.semantic-record-malformed` | B | Records lack a typed model representation. |
| `pmi.swift-annotation-unsupported` | B | Records lack a typed model representation. |
| `pmi.swift-feature-walk-capped` | B | Records lack a typed model representation. |
| `sketch.native-constraint` | C | Typed neutral or native records retain the reduced content. |
| `sketch.native-geometry` | C | Typed neutral or native records retain the reduced content. |
| `sketch.profile-rejected` | A | Class/name framing and raw payload do not retain semantic operands. |
| `sketch.relation-multiply-projected` | C | Typed neutral or native records retain the reduced content. |
| `sketch.relation-unprojected` | A/C | C only when every relation has typed operands or a complete typed instance; otherwise A. |
| `source.dialect-layer-collision` | D | Identity, framing, or report metadata. |
| `source.dialect-unverified` | D | Identity, framing, or report metadata. |
| `source.kernel-dialect-unverified` | D | Identity, framing, or report metadata. |
| `source.opaque-records-unrepresented` | A+B | A+B: source sections outside the typed family allowlist survive only as bytes. |
| `source.preserved-image-unavailable` | (w) | Export-only; does not affect decode fidelity. |
| `source.property-key-blank` | B | Records lack a typed model representation. |
| `target.source-dialect-displaced` | (w) | Export-only; does not affect decode fidelity. |
| `tessellation.face-ownership-unresolved` | C | Typed neutral or native records retain the reduced content. |
| `tessellation.owner-work-capped` | C | Typed neutral or native records retain the reduced content. |
| `topology.body-hierarchy-derived` | C | Typed neutral or native records retain the reduced content. |
| `topology.face-owner-ambiguous` | C | Typed neutral or native records retain the reduced content. |
| `topology.face-sense-conflict` | C | Typed neutral or native records retain the reduced content. |
| `topology.face-unclaimed` | B | Records lack a typed model representation. |
| `topology.graph-not-transferred` | B | Records lack a typed model representation. |
| `topology.pcurve-carrier-off-surface` | B | Records lack a typed model representation. |
| `writer.appearance-bindings-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.arena-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.attributes-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.brep-not-regenerated` | (w) | Export-only; does not affect decode fidelity. |
| `writer.coedge-pcurve-uses-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.design-reduced` | (w) | Export-only; does not affect decode fidelity. |
| `writer.entity-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.fields-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.native-records-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.procedural-reduced` | (w) | Export-only; does not affect decode fidelity. |
| `writer.shell-wire-topology-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.sphere-seam-curves-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.tessellation-metadata-omitted` | (w) | Export-only; does not affect decode fidelity. |
| `writer.unbound-appearances-omitted` | (w) | Export-only; does not affect decode fidelity. |

## STEP

| Local wire code | Class | Retention decision |
|---|---|---|
| `appearance.binding-metadata-reduced` | (w) | Appearance bindings carry source object or channel metadata. Export-only code; does not change decode fidelity. |
| `appearance.binding-missing-asset` | (w) | Appearance bindings reference missing appearance assets. Export-only code; does not change decode fidelity. |
| `appearance.binding-no-base-color` | (w) | Appearance bindings reference appearances without a base color. Export-only code; does not change decode fidelity. |
| `appearance.binding-target-conflict` | (w) | Appearance bindings target one body or face with conflicting styles. Export-only code; does not change decode fidelity. |
| `appearance.display-color-unstyled` | (w) | Display colors had no emitted STEP item. Export-only code; does not change decode fidelity. |
| `appearance.reduced-to-base-color` | (w) | Appearance assets were reduced to STYLED_ITEM base colors. Export-only code; does not change decode fidelity. |
| `appearance.wire-body-transparency-omitted` | (w) | A wire body's direct color alpha was omitted from RGB curve styling. Export-only code; does not change decode fidelity. |
| `assembly.graph-invalid` | (w) | The assembly occurrence graph is invalid. Export-only code; does not change decode fidelity. |
| `assembly.joint-omitted` | (w) | Assembly joints were not represented. Export-only code; does not change decode fidelity. |
| `assembly.occurrence-omitted-no-parent-product` | (w) | Assembly occurrences were omitted because the parent has no local product. Export-only code; does not change decode fidelity. |
| `asset.document-omitted` | (w) | Document assets were not represented. Export-only code; does not change decode fidelity. |
| `attribute.source-record-not-written` | (w) | Source attribute records were not written. Export-only code; does not change decode fidelity. |
| `attribute.string-invalid` | B | An attribute string field failed to decode. The omitted record payload has no typed document representation. |
| `body.hidden-omitted` | (w) | Hidden bodies had no emitted STEP item and were omitted from INVISIBILITY. Export-only code; does not change decode fidelity. |
| `body.hidden-visibility-unsupported` | (w) | Hidden body visibility assignments are unsupported by the target schema. Export-only code; does not change decode fidelity. |
| `body.non-rigid-transform` | (w) | A body carries a non-rigid transform. Export-only code; does not change decode fidelity. |
| `container.member-noncanonical` | D | ZIP metadata violates the Part 21 profile without preventing root decoding. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `decode.byte-accounting-unclassified` | B | Byte accounting left source bytes unclassified. The omitted record payload has no typed document representation. |
| `decode.opaque-record-preserved` | B | Named entity instances were retained as opaque records. The omitted record payload has no typed document representation. |
| `decode.warning` | B | A decode stage surfaced a per-record warning. The omitted record payload has no typed document representation. |
| `design.parametric-records-omitted` | (w) | Parametric or design records were not represented. Export-only code; does not change decode fidelity. |
| `drawing.draughting-associated-item-untyped` | B | A draughting model association references a source-typed item without identity. The omitted record payload has no typed document representation. |
| `drawing.draughting-semantic-definition-untyped` | B | A draughting model association references a typed semantic definition without identity. The omitted record payload has no typed document representation. |
| `drawing.order-unstatable` | B | A drawing's position in the stored order passes the stated order width. The omitted record payload has no typed document representation. |
| `drawing.record-too-few-parameters` | B | A drawing record has too few parameters and was retained opaque. The omitted record payload has no typed document representation. |
| `drawing.relationship-target-ambiguous` | B | A drawing relationship references a source record with multiple identities. The omitted record payload has no typed document representation. |
| `drawing.relationship-untyped-target` | B | A drawing relationship references a source-typed record without identity. The omitted record payload has no typed document representation. |
| `drawing.revision-sheet-unresolved` | B | A drawing revision usage has no resolvable sheet revision. The omitted record payload has no typed document representation. |
| `drawing.sheet-revision-unresolved` | B | A drawing sheet usage has no resolvable drawing revision. The omitted record payload has no typed document representation. |
| `export.no-exportable-solids` | (w) | The IR document has bodies but no exportable solids. Export-only code; does not change decode fidelity. |
| `geometry.analytic-surface-normalized` | (w) | Signed or self-intersecting analytic surfaces were normalized. Export-only code; does not change decode fidelity. |
| `geometry.carrier-not-written` | (w) | Geometry carriers were not written. Export-only code; does not change decode fidelity. |
| `geometry.conflicting-representation-units` | B | Geometry records belong to representations with conflicting units. The omitted record payload has no typed document representation. |
| `geometry.curveless-edge-omitted` | (w) | Edges have no typed 3D curve or carry an unsupported transform. Export-only code; does not change decode fidelity. |
| `geometry.document-angle-unit-unresolved` | D | The document plane-angle unit did not resolve. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.document-length-unit-unresolved` | D | The document length unit did not resolve. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.elliptical-cone-reduced` | (w) | Elliptical cone surfaces were reduced to circular CONICAL_SURFACE. Export-only code; does not change decode fidelity. |
| `geometry.line-parameter-scale-unresolved` | B | A LINE parameter scale did not resolve. The omitted record payload has no typed document representation. |
| `geometry.placement-reference-inferred` | C | AXIS2_PLACEMENT_3D reference direction was parallel to its axis. The source record survives as typed content; its neutral fidelity is reduced. |
| `geometry.procedural-definition-not-written` | (w) | A procedural definition has no emitted native construction or solved carrier. Export-only code; does not change decode fidelity. |
| `geometry.procedural-reduced-to-carrier` | (w) | Procedural definitions were reduced to solved STEP carriers. Export-only code; does not change decode fidelity. |
| `geometry.subd-omitted` | (w) | Subdivision surfaces were omitted. Export-only code; does not change decode fidelity. |
| `geometry.uncertainty-length-ambiguous` | D | GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT has an ambiguous length measure. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.uncertainty-length-unresolved` | D | GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT has no resolvable length measure. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `geometry.unknown-surface-face-omitted` | (w) | Faces rest on an unknown or STEP-unsupported surface. Export-only code; does not change decode fidelity. |
| `layer.item-without-carrier` | (w) | A layer item has no writable STEP carrier. Export-only code; does not change decode fidelity. |
| `metadata.schema-object-identifier-out-of-range` | D | A FILE_SCHEMA object identifier component is out of range. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `metadata.string-invalid` | D | A metadata string field failed to decode. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `occurrence.expansion-depth-truncated` | B | Product occurrence expansion stopped a branch at its path-depth limit. The omitted record payload has no typed document representation. |
| `occurrence.expansion-output-truncated` | B | Product occurrence expansion stopped at its occurrence limit. The omitted record payload has no typed document representation. |
| `occurrence.external-product` | (w) | Occurrences reference external product definitions. Export-only code; does not change decode fidelity. |
| `occurrence.metadata-omitted` | (w) | Occurrence metadata values were not represented. Export-only code; does not change decode fidelity. |
| `occurrence.no-local-product` | (w) | An occurrence has no local product definition. Export-only code; does not change decode fidelity. |
| `occurrence.no-writable-product` | (w) | Occurrences have no writable product definition. Export-only code; does not change decode fidelity. |
| `occurrence.placement-not-rigid` | (w) | An occurrence placement is not rigid. Export-only code; does not change decode fidelity. |
| `occurrence.root-placement-not-representable` | (w) | A root occurrence placement or scale is not representable. Export-only code; does not change decode fidelity. |
| `occurrence.unresolved-parent` | (w) | An occurrence has an unresolved parent. Export-only code; does not change decode fidelity. |
| `parse.draft-grammar` | D | A pre-standard exchange used the Part 21 instance grammar. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.envelope-incomplete` | D | Readable DATA ends before a complete closing envelope. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.expansion-bounded` | B | An anchor or REFERENCE expansion passed its value-node limit; the binding or instance remains source-only. The omitted record payload has no typed document representation. |
| `parse.header-metadata-noncanonical` | D | Header metadata deviates from the format without preventing decoding. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.implementation-level-unverified` | D | The declared Part 21 implementation level has no implemented grammar. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.noncanonical-syntax` | D | Parser recovered noncanonical Part 21 syntax. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.preamble-omitted` | D | A bounded noncanonical prefix remains source-only. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `parse.record-omitted` | B | A bounded DATA record or its dependent could not be interpreted. The omitted record payload has no typed document representation. |
| `pcurve.carrier-unwritable` | (w) | Coedge pcurve carriers use unwritable geometry or surface references. Export-only code; does not change decode fidelity. |
| `pcurve.coedge-native-metadata` | (w) | Emitted coedge pcurves carry native-only metadata. Export-only code; does not change decode fidelity. |
| `pcurve.coedge-no-geometry` | (w) | Coedge pcurve references have no geometry. Export-only code; does not change decode fidelity. |
| `pcurve.use-native-metadata` | (w) | Pcurve uses carry native-only parameter metadata. Export-only code; does not change decode fidelity. |
| `pmi.angle-unit-unresolved` | B | A PMI angle measure unit scale did not resolve. The omitted record payload has no typed document representation. |
| `pmi.annotation-not-written` | (w) | PMI annotations were not written. Export-only code; does not change decode fidelity. |
| `pmi.datum-system-invalid` | B | A datum system has inconsistent precedence compartments. The omitted record payload has no typed document representation. |
| `pmi.dimensional-nominal-ambiguous` | B | A dimensional characteristic has several named nominal measures. The omitted record payload has no typed document representation. |
| `pmi.dimensional-unnamed-measure-ambiguous` | B | A dimensional characteristic has several unnamed measure values. The omitted record payload has no typed document representation. |
| `pmi.hidden-visibility-unsupported` | (w) | Hidden PMI annotation visibility assignments are unsupported by the target schema. Export-only code; does not change decode fidelity. |
| `pmi.length-unit-unresolved` | B | A PMI length measure unit scale did not resolve. The omitted record payload has no typed document representation. |
| `pmi.presentation-annotation-placement-ambiguous` | B | A presentation annotation has several placement carriers and no unique placement. The omitted record payload has no typed document representation. |
| `pmi.presentation-annotation-text-unordered` | B | A presentation annotation has several text carriers and no order. The omitted record payload has no typed document representation. |
| `pmi.semantic-annotation-omitted` | (w) | Semantic annotations were not represented. Export-only code; does not change decode fidelity. |
| `presentation.conflicting-scalar-colors` | C | Independent styled items assign conflicting scalar colors. The source record survives as typed content; its neutral fidelity is reduced. |
| `presentation.context-dependent-style-unresolved` | B | A context-dependent style has no matching neutral presentation context. The omitted record payload has no typed document representation. |
| `presentation.drawing-records-omitted` | (w) | Drawing or presentation records were not represented. Export-only code; does not change decode fidelity. |
| `presentation.hidden-appearance-visibility-unsupported` | (w) | Hidden appearance binding visibility assignments are unsupported by the target schema. Export-only code; does not change decode fidelity. |
| `presentation.hidden-layer-omitted` | (w) | Hidden presentation layers had no emitted STEP assignment and were omitted from INVISIBILITY. Export-only code; does not change decode fidelity. |
| `presentation.hidden-layer-visibility-unsupported` | (w) | Hidden presentation layer visibility assignments are unsupported by the target schema. Export-only code; does not change decode fidelity. |
| `presentation.surface-side-invalid` | C | A surface style usage has an invalid surface_side enumeration value. The source record survives as typed content; its neutral fidelity is reduced. |
| `presentation.surface-transparency-conflict` | C | A surface rendering has multiple transparency properties. The source record survives as typed content; its neutral fidelity is reduced. |
| `product.body-conflicting-mapped-placements` | B | A body has conflicting standalone MAPPED_ITEM placements. The omitted record payload has no typed document representation. |
| `product.body-without-representation` | (w) | Bodies have no owning product representation. Export-only code; does not change decode fidelity. |
| `product.bom-property-omitted` | (w) | Product BOM property values were not represented. Export-only code; does not change decode fidelity. |
| `product.nauo-placement-ambiguous` | B | A NAUO has competing resolved placement candidates. The omitted record payload has no typed document representation. |
| `product.nauo-placement-unresolved` | B | A NAUO has no resolved occurrence transform. The omitted record payload has no typed document representation. |
| `product.non-part-kind` | (w) | Product definitions use a non-part kind. Export-only code; does not change decode fidelity. |
| `source.association-omitted` | (w) | Source-object associations were not represented. Export-only code; does not change decode fidelity. |
| `source.dialect-unverified` | D | The declared FILE_SCHEMA identifier satisfies no declared dialect. This affects source/envelope/diagnostic metadata rather than an arena record. |
| `target.source-dialect-displaced` | (w) | The selected write target differs from the same-format source dialect. Export-only code; does not change decode fidelity. |
| `tessellation.body-link-unwritable` | (w) | A tessellation body has no writable AP242 tessellation link. Export-only code; does not change decode fidelity. |
| `tessellation.corner-normals` | (w) | Tessellation corner normals are not represented. Export-only code; does not change decode fidelity. |
| `tessellation.feature-edges` | (w) | Tessellation feature-edge classification is not represented. Export-only code; does not change decode fidelity. |
| `tessellation.invalid-cardinality` | (w) | A tessellation has invalid vertex, index, or normal cardinality. Export-only code; does not change decode fidelity. |
| `tessellation.invalid-payload` | B | A tessellation payload fails numeric or structural admission. The omitted record payload has no typed document representation. |
| `tessellation.item-body-unresolved` | B | A body-representation tessellation item does not bind to exactly one decoded body. The omitted record payload has no typed document representation. |
| `tessellation.item-undeclared` | B | A tessellation item lacks an exact body-container or tessellated-representation declaration. The omitted record payload has no typed document representation. |
| `tessellation.metadata-reduced` | (w) | Tessellation metadata was reduced because STEP cannot represent it. Export-only code; does not change decode fidelity. |
| `tessellation.placement-ambiguous` | B | A detached tessellation item has multiple distinct repositioning placements. The omitted record payload has no typed document representation. |
| `tessellation.placement-unresolved` | B | A repositioned tessellation item has no valid placement. The omitted record payload has no typed document representation. |
| `tessellation.requires-ap242` | (w) | Tessellations require an AP242 target. Export-only code; does not change decode fidelity. |
| `tessellation.texture-assignments` | (w) | Tessellation texture assignments are not represented. Export-only code; does not change decode fidelity. |
| `tessellation.triangle-groups` | (w) | Tessellation triangle groups are not represented. Export-only code; does not change decode fidelity. |
| `topology.coedge-use-curve-not-represented` | (w) | Coedge-local 3D curve uses were not represented. Export-only code; does not change decode fidelity. |
| `topology.edge-curve-not-writable` | (w) | An edge was omitted because its curve is not writable. Export-only code; does not change decode fidelity. |
| `topology.edge-curve-unsupported` | (w) | An edge was omitted because its curve is unsupported. Export-only code; does not change decode fidelity. |
| `topology.edge-end-vertex-missing` | (w) | An edge was omitted because its end vertex is missing. Export-only code; does not change decode fidelity. |
| `topology.edge-no-3d-curve` | (w) | An edge was omitted because it has no 3D curve. Export-only code; does not change decode fidelity. |
| `topology.edge-no-surface-or-curve-for-pcurve` | B | An edge has no decoded surface or curve carrier for a pcurve. The omitted record payload has no typed document representation. |
| `topology.edge-record-missing` | (w) | An edge was omitted because its record is missing. Export-only code; does not change decode fidelity. |
| `topology.edge-start-vertex-missing` | (w) | An edge was omitted because its start vertex is missing. Export-only code; does not change decode fidelity. |
| `topology.face-multiple-outer-bounds` | C | A face has more than one FACE_OUTER_BOUND. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.face-no-writable-bounds` | (w) | A face has no writable bounds. Export-only code; does not change decode fidelity. |
| `topology.face-omitted-inner-loop` | (w) | A required inner loop has no writable topology; structural degradation is reported at Error severity. Export-only code; does not change decode fidelity. |
| `topology.face-omitted-outer-loop` | (w) | A face omitted an outer loop with no writable topology. Export-only code; does not change decode fidelity. |
| `topology.face-omitted-unclassified-loop` | (w) | A face omitted a loop whose boundary role is unspecified. Export-only code; does not change decode fidelity. |
| `topology.loop-coedge-missing-for-order` | (w) | A loop cannot be ordered because a coedge is missing. Export-only code; does not change decode fidelity. |
| `topology.loop-coedge-record-missing` | (w) | A loop omitted a coedge because its record is missing. Export-only code; does not change decode fidelity. |
| `topology.loop-duplicate-coedge` | (w) | A loop cannot be ordered because a coedge occurs more than once. Export-only code; does not change decode fidelity. |
| `topology.loop-edge-missing-for-order` | (w) | A loop cannot be ordered because an edge is missing. Export-only code; does not change decode fidelity. |
| `topology.loop-edge-not-writable` | (w) | A loop omitted an edge because the edge is not writable. Export-only code; does not change decode fidelity. |
| `topology.loop-no-continuous-ordering` | (w) | A loop has no continuous vertex-to-vertex coedge ordering. Export-only code; does not change decode fidelity. |
| `topology.loop-no-writable-edges` | (w) | A loop has no writable edges. Export-only code; does not change decode fidelity. |
| `topology.loop-record-missing` | (w) | A loop was omitted because its record is missing. Export-only code; does not change decode fidelity. |
| `topology.loop-vertex-missing` | (w) | A loop was omitted because its vertex is missing. Export-only code; does not change decode fidelity. |
| `topology.metadata-not-represented` | (w) | Topology metadata values were not represented. Export-only code; does not change decode fidelity. |
| `topology.non-manifold-edge-split` | (w) | A non-manifold edge of a closed shell was written as one edge per forward and reversed use pair. Export-only code; does not change decode fidelity. |
| `topology.oriented-shell-omits-cfs-faces` | C | An oriented shell omits the derived cfs_faces slot. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.pcurve-association-ambiguous` | B | Several pcurves associate with a surface and none selects uniquely. The omitted record payload has no typed document representation. |
| `topology.pcurve-candidates-carrier-unresolved` | B | Pcurve candidates exist but the source surface or curve is unresolved. The omitted record payload has no typed document representation. |
| `topology.pcurve-endpoints-discontinuous` | B | A single optional pcurve is not endpoint-continuous with the edge. The omitted record payload has no typed document representation. |
| `topology.pcurve-global-fidelity-unproved` | B | A finite non-seam pcurve witness did not prove global point-set and direction fidelity. The omitted record payload has no typed document representation. |
| `topology.pcurve-locus-discontinuous` | B | A single endpoint-continuous pcurve fails the bounded model-space locus witness. The omitted record payload has no typed document representation. |
| `topology.region-no-shell-list` | (w) | Regions have no shell list and were not written. Export-only code; does not change decode fidelity. |
| `topology.region-no-writable-outer-shell` | (w) | A region has no writable outer shell. Export-only code; does not change decode fidelity. |
| `topology.region-omitted-void-shell` | (w) | A region omitted a void shell with no writable faces. Export-only code; does not change decode fidelity. |
| `topology.root-incomplete` | B | A topology root does not resolve to a complete connected graph. The omitted record payload has no typed document representation. |
| `topology.root-rejected` | B | A topology root was rejected with a failure message. The omitted record payload has no typed document representation. |
| `topology.seam-edge-pcurve-unresolved` | B | A SEAM_EDGE has no decoded pcurve on its edge curve and face surface. The omitted record payload has no typed document representation. |
| `topology.shell-disconnected-faces` | C | A source shell contains disconnected face components. The source record survives as typed content; its neutral fidelity is reduced. |
| `topology.shell-omitted-outer-face` | (w) | A shell omitted an outer face with no writable topology. Export-only code; does not change decode fidelity. |
| `topology.shell-omitted-unclassified-face` | (w) | A shell omitted a face whose boundary roles are unspecified. Export-only code; does not change decode fidelity. |
| `topology.surface-shell-wire-omitted` | (w) | Solid and sheet shells omit wire edges and free vertices. Export-only code; does not change decode fidelity. |
| `topology.unreachable-from-region` | (w) | Topology is not reachable from any emitted region shape item. Export-only code; does not change decode fidelity. |
| `topology.wire-region-missing-shell` | (w) | Wire region or shell relations referenced missing shell records. Export-only code; does not change decode fidelity. |
| `topology.wire-region-no-connected-edge-set` | (w) | Wire regions had no writable connected edge set. Export-only code; does not change decode fidelity. |
| `topology.wire-shell-free-vertices` | (w) | A wire shell has free vertices without an edge-based STEP carrier. Export-only code; does not change decode fidelity. |
| `validation.measure-unit-unresolved` | B | A geometric validation measure unit scale did not resolve. The omitted record payload has no typed document representation. |

