# CATIA semantic control cross-walks

Date: 2026-08-21
Update: 2026-08-25

This report records six sanitized control documents as `C06` through `C10` and
`CH`. It uses the companion PDF text, rendered 3D pages, and the feature-list
slides for `CH`. The release CATIA decoder was rebuilt before the sweep. Each
input was decoded separately with a 240-second limit and all six completed with
status 0.

The companion text for `C06`--`C10` names saved-view configurations and units,
but does not contain an operation-level feature list. Their rendered pages show
the model notes, direct and basic dimensions, datum identifiers, and feature
control frames. An exact callout-to-entity list is therefore open for those
controls; it is not inferred from the rendered labels. The `CH` feature list
names each operation and its annotation views, so it has an operation-level
cross-walk below.

All offsets in this report are zero-based offsets in the source file. An
`inner` offset is the beginning of the nested B-rep stream. A `field offset` is
the first field offset of a native design object that carries the named class.
The report records offsets and decoded class names only; it does not retain
source bytes.

## Decode census

| control | inner offset | B-rep bytes | FBB face rows | vertex records | model faces / edges | model features | model parameters | model PMI | result |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| C06 | 2290936 | 79405 | 187 | 311 | 187 / 476 | 32 unresolved extrusions | 0 | 0 | geometry and topology transferred; one unresolved curve carrier |
| C07 | 1137656 | 93699 | 304 | 582 | 304 / 865 | 0 | 0 | 0 | geometry and topology transferred; 20 unresolved face-local carriers |
| C08 | 1699598 | 78473 | 270 | 443 | 270 / 705 | 2 unresolved datum planes | 0 | 0 | geometry and topology transferred; two unresolved datum planes |
| C09 | 1533146 | 52377 | 158 | 300 | 158 / 454 | 0 | 31 `Length` values | 0 | geometry and topology transferred |
| C10 | 1410531 | 89049 | 264 | 560 | 264 / 799 | 0 | 8 `TolNumValue` values | 0 | geometry and topology transferred; one unresolved surface and four curve carriers |
| CH | 1360565 | 47056 | 159 | 272 | 159 / 409 | 18 unresolved datum planes | 0 | 0 | geometry and topology transferred |

All six report `variant=standard_nested`. The zero PMI and configuration
counts are neutral-model counts, not claims that the source has no annotation
or view records. Native records for those subjects are listed below.

## C06--C10 companion cross-walk

The saved-view names below are the complete names exposed by the companion text
layer. The visible annotation categories are the categories that can be
identified in the rendered pages without assigning a text label to a source
entity.

### C06

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Saved views `MBD_A`, `MBD_B`, `MBD_C` | Native `MBD_A_B_C`, `CATTPSViewTransfData`, `CATTPSViewRatioData`, and `TTRSSet` classes are retained. `model.configurations=0`. | partial — native view records occur at field offsets 1202658, 1202777, and 1202941; document configuration ownership is not transferred. |
| Model notes and directly toleranced/basic dimensions shown in the pages | Native `Geom_for_MBD_A_notes`, `TextBloc`, `HasADatum`, and `TTRSSet` classes are retained. `model.pmi=0`. | partial — field offsets 1205204, 1206235, and 1211097 retain annotation-related classes, but no neutral annotation is emitted. |
| Feature-control frames and datum identifiers visible in the pages | Datum and presentation class evidence is retained with the view records. | open: exact frame-to-face and datum-to-feature ownership is not identified from the admitted relations at field offsets 1205204--1211097. |
| Operation-level feature list | The companion text does not state one. The decoder emits 32 `extrude_unresolved` features; native operation classes include `Prism_ThickThin1`, `EdgeFillet_Radius`, `Hole_ListDTable`, `Counterbored Hole`, and `InternalPattern`. | partial — native operation vocabulary is retained at field offsets 1201859, 1204203, 1206235, and 1206966, but operation roles and replay order are not transferred. |

### C07

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Saved views `MBD_A`, `MBD_B`, `MBD_C`, `MBD_D` | Native view and dimension classes are retained. `model.configurations=0`. | partial — `DimInst` begins at field offsets 1318578, 1318611, 1318640, and 1318667; the view/configuration owner is not transferred. |
| Direct/basic dimensions and datum frames visible in the pages | `DimInst`, `DimLine`, `DimValCompDual`, `DimValStyle`, `RepresentedTTRS`, and `ReferencePlaneTTRS` are retained. `model.pmi=0`. | partial — field offsets 1318578, 1319845, and 405430 retain typed native evidence, but no neutral PMI or datum relation is emitted. |
| Notes, surface requirements, and feature-control frames visible in the pages | Native note and tolerance-related records remain in design-object fields; the 304-face topology is emitted. | open: exact annotation ownership cannot be assigned; 20 face-local free-form carriers remain unresolved at inner offset 1137656. |
| Operation-level feature list | The companion text does not state one. No neutral feature is emitted. | missing — native records retain 84 design objects, but no admitted feature instance reaches `model.features`; topology is no longer the blocking stage. |

### C08

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Saved views `MBD_A`, `MBD_B`, `MBD_C`, `MBD_D` | Native view/presentation records are retained. `model.configurations=0`. | partial — view-related value records occur in the 85 design objects; no configuration owner is transferred. |
| Dimensions, datum identifiers, and feature-control frames visible in the pages | Native classes include `GSMPlaneOffset`, `CstAttr_Side`, and `Range`; `model.pmi=0`. | partial — field offsets 769194, 769912, 770733, and 822528 retain the relevant class evidence; no neutral PMI is emitted. |
| Hole, pattern, fillet, and limiting-element intent visible in the model and pages | Native classes include `RectPattern_Nb2`, `RectPattern_DesignIntent`, `Hole_SensThread`, `Hole_Pas`, `HoleType`, `FilletLimitingElementList`, and `EdgeFillet`; neutral output is two unresolved datum planes. | partial — field offsets 769912, 770084, 770733, and 773884 retain operation vocabulary without operation instances. |
| Operation-level feature list | The companion text does not state one. | open: operation-to-output binding is not assigned; the 270-face topology transfers, but two unresolved datum-plane features remain. |

### C09

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Saved views `MBD_A`, `MBD_B`, `MBD_C`, `MBD_D` | Native view blocks and capture records are retained. `model.configurations=0`. | partial — `ViewBloc`, `TTRSViewBloc`, and capture classes occur at field offsets 731074, 732616, 742934, and 743034. |
| Direct/basic dimensions, datum frames, and feature-control frames visible in the pages | Native `CATTPSDimensionData`, `CATTPSDatumData`, `CATTPSToleranceWithDRF`, and display classes are retained. `model.pmi=0`. | partial — field offsets 725993, 726759, 730648, and 741599 retain the source classes but no neutral annotation. |
| Operation intent visible in the model/pages | Native `Pad`, `Chamfer`, `ThickThin1`, `HoleLimitType`, `PatternSpacing`, `StaggerAngle`, and `BasicDim` classes are retained. `model.features=0`. | partial — field offsets 725993, 730527, 731941, and 732616 retain operation/value vocabulary; no feature instance is emitted. |
| Dimensional values | 31 neutral parameters named `Length` are emitted, including values with angular-looking magnitudes. No dimensional subtype is assigned. | partial — `CATTPSDimensionData` and `CstAttr_Crv2Param` occur at field offsets 725993 and 726759; quantity semantics remain open. |

### C10

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Saved views `MBD_A`, `MBD_B`, `MBD_C`, `MBD_D`, `MBD_E` | Native `BuildConfiguration` and `View` evidence is retained. `model.configurations=0`. | partial — field offset 1638628 carries the configuration/view class; no neutral configuration is emitted. |
| Direct/basic dimensions, datum identifiers, parallelism and feature-control frames visible in the pages | Native `CstAttr_Parallelism`, `CstAttr_Side`, `DatumTheoExactBloc`, `CATTPSDatum`, and drawing-text classes are retained. `model.pmi=0`. | partial — field offsets 592106, 592146, 1638479, and 1639814 retain annotation classes without neutral PMI. |
| Tolerance values | Eight neutral `TolNumValue` parameters are emitted. They have no typed PMI owner or quantity. | partial — native tolerance/configuration fields occur at offsets 592815 and 1638479; no neutral dimension or tolerance annotation is emitted. |
| Operation-level feature list | The companion text does not state one. No neutral feature is emitted. | open: operation-to-face ownership is not assigned; the 264-face topology transfers, while one surface and four curve carriers remain unresolved. |

## CH operation cross-walk

The feature-list slides explicitly describe the following operations. A
`partial` row means that native class evidence survives but the named operation
does not become a typed neutral feature. A `missing` row has neither a direct
native class match nor a typed neutral feature. `model.features=18` consists
only of `datum_plane_unresolved` records at field offsets 592862--599869.

| documented feature | decoder result | status and byte evidence |
| --- | --- | --- |
| `Base` — centered extrude | Native `Base`; no typed extrude. | partial — field offset 580441. |
| `Raised_Base` — offset extrude | Native `Raised_Base`; no typed extrude. | partial — field offset 581871. |
| `Ramp` — extruded cut | No direct `Ramp` class or neutral cut. | missing — the native feature scan covers 122 design objects; the adjacent operation group begins at field offset 580441. |
| `Vertical_Edges_Blend` — fillet | Native `FilletType`; no typed fillet. | partial — field offset 580441. |
| `Chamfer` — symmetric chamfer | No direct chamfer feature or neutral chamfer. | missing — no direct class match in the 122 design objects; feature-group evidence begins at field offset 580441. |
| `Clip` — bidirectional extrude | No direct clip feature or neutral extrude. | missing — no direct class match; related feature group begins at field offset 580441. |
| `Clip_Mirror` — mirror on XZ | Native `Clip_Mirror` and `FeatureREDGE`; no typed mirror. | partial — field offsets 580441 and 580885. |
| `1-THRU-ALL` — concentric through-all hole | Native hole value classes; no typed hole. | partial — `Hole_BottomAngle` and `Hole_CBDiameter` occur at field offsets 580441 and 582720. |
| `2-LINEAR_PATTERN` — Y-direction pattern | No direct linear-pattern class or neutral pattern. | missing — no direct class match; related operation fields begin at field offset 580441. |
| `3-NUMERIC_THRU` — finite-depth hole | Native hole value classes; no typed hole. | partial — field offsets 580441 and 584416. |
| `4-SKETCH_EXTRUDE_THRU_ALL` — sketch extrude | Native `Pad`; no typed sketch/extrude feature. | partial — field offset 580441. |
| `5-REVOLVE_THRU_ALL` — revolved cut | Native `ThickThin2`; no typed revolution. | partial — field offsets 580200 and 584416. |
| `6-COUNTERBORE_THRU` — counterbored hole | Native `HoleCounterBoredType`; no typed hole. | partial — field offset 580441. |
| `7-COUNTERBORE_DRILL` — counterbored hole | Native `HoleCounterBoredType`; no typed hole. | partial — field offsets 580525 and 580616. |
| `CIRCULAR_PATTERN` — annotation-authoring sketch | No direct sketch or typed pattern. | missing — native annotation/view fields begin at field offset 580441; `model.sketch_constraints=0`. |
| `8-COUNTERSUNK_DRILL` — countersunk hole | Native `Hole_CSMode`; no typed hole. | partial — field offset 580103. |
| `9-CIRCULAR_PATTERN` — circular pattern | No direct circular-pattern class or neutral pattern. | missing — no direct class match; related operation fields begin at field offset 580441. |
| `10-PARTIAL_ANGLED_SURFACE` — partial circular pattern | Native `FeatureFSUR` and orientation fields; no typed pattern. | partial — field offsets 582664 and 580441. |
| `11-COUNTERBORE_ANGLED_SURFACE` — counterbored hole | Native `HoleCounterBoredType`; no typed hole. | partial — field offset 580441. |
| `12-CONVEX_3_SURFACES` — multi-surface hole | Native `FeatureFSUR`/`FeatureREDGE`; no typed hole. | partial — field offsets 582664 and 580885. |
| `13-ANGLE` — angled hole | Native direction/orientation classes; no typed hole. | partial — `CATTPSDirectionFeatureDataBloc` at field offset 592862 and `CstAttr_Orientation` at 580441. |
| `14-PARTIAL_HOLE` — partial hole | Native hole value classes; no typed hole. | partial — field offsets 580103 and 580441. |
| `Mirror Feature` — mirror on XZ | Native mirror-related class evidence; no typed mirror. | partial — `Clip_Mirror` and `FeatureREDGE` at field offsets 580441 and 580885. |
| `15-THRU_SELECTED` — up-to-surface hole | Native surface-feature evidence; no typed up-to-surface hole. | partial — `FeatureFSUR` at field offset 582664. |
| `Logo` — extrude | No direct logo, sketch, or typed extrude. | missing — no direct class match in the native design-object scan; feature-group evidence begins at field offset 580441. |

## CH annotations and saved views

| documented item | decoder result | status and byte evidence |
| --- | --- | --- |
| Six individual notes and one feature-control frame | Native `01_NOTES`, `ReportNote`, `TextBloc`, `DimTolMain`, `CATTPSDimensionData`, and tolerance classes are retained. `model.pmi=0`. | partial — field offsets 580441, 586535, 1521222, and 580885 retain the records; note and frame ownership is not transferred. |
| Datum view and datum features | Native `02_SET_DATUMS`, `CATTPSDatumCFData`, and `CATTPSDatum` classes are retained. | partial — field offsets 580441, 592862, and 601345; no neutral datum/PMI owner is emitted. |
| Saved views `00_MODEL_ONLY_FRONT`, `00_MODEL_ONLY_BACK`, `01_NOTES`, `02_DATUMS`, `03_FRONT_LEFT`, `04_FRONT_RIGHT`, `05_BACK_UNDER`, `06_TOP_LOWER`, `07_SLOPE_FRONT`, `08_SLOPE_SECTION`, `09_LEFT` | Native view-index and view-data classes are retained. `model.configurations=0`. | partial — `ViewIdxBloc` and `CATTPSViewDataUpgrade01` occur at field offsets 592862 and 597464; document configuration ownership is not transferred. |

## Prior-item verdicts

These are the one-line re-decisions made from the six controls. The same
verdicts are appended to the corresponding open items.

- **DI-13:** remains open — named saved views and native configuration/view classes occur at C06 field offsets 1202658/1202777/1202941 and C10 offset 1638628, but all six neutral documents have `model.configurations=0`.
- **DI-18:** remains open — C06 retains 14 complete `Range` intervals but zero finite nominal values, while C08 exposes `Range` class evidence at field offset 822528; no owner relation reaches a transferred sketch, feature, or PMI object.
- **DI-22:** remains open — C07 retains `DimInst`/`DimLine` at field offsets 1318578/1319845 and C09 retains `CATTPSDimensionData` at 725993/726759, but both emit `model.sketch_constraints=0`.
- **DI-23:** remains open — C06 and CH retain operation class vocabulary at field offsets 1201859/1204203 and 580441/584416, but the neutral operation output is unresolved datum/extrusion evidence rather than typed operation instances.
- **DI-24:** remains open — C09 emits 31 untyped `Length` parameters and C10 emits 8 untyped `TolNumValue` parameters while both have `model.pmi=0`; native dimensional/tolerance classes occur at C09 offsets 725993/730648 and C10 offsets 592815/1638479.
- **SN-37:** remains open — the current release route transfers the 304-, 270-, and 264-face topologies at inner offsets 1137656, 1699598, and 1410531. C07 and C08 retain no open alternate second-face domains; C10 retains 52 repeated rows, but each allowed second-face domain is singleton before the joint solver. The sibling-neutral models have different face/edge cardinalities (306/871, 271/709, and 256/551), so they confirm split-policy differences but do not supply the allocation-scoped identity bridge required by SN-37.

## Next discriminating work

The face-domain comparison is complete. It confirms that `allowed_faces` must
reach the joint solver when repeated rows remain, and that the current C10
assignment closes to singleton domains. It does not identify the source rule
for the fixed-nine owner boundary cycle in SN-37. The next SN-37 check must
therefore use a source-closed allocation bridge or retain the cycle without a
standard-face join. Feature-history and saved-view ownership remain separate
DI-13/DI-23/DI-28 work; the neutral topology witness cannot settle those
design-intent questions.
