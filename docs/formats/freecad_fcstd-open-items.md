# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 3. Persistent topology identity

## 4. Exact-topology transfer

## 5. Design projection

### DP-15. Design operation selector fallback

**Question.** How are absent and malformed design operation mode or flag properties distinguished
before selecting a neutral operation family?

**Known.** PartDesign Boolean, Revolution, and Groove `Type` carriers are settled. The producer
registers each as an `App::PropertyEnumeration` with a constructor default of index `0`; its
writer emits one direct `Integer` value. `design.rs:1906-1923` now supplies the default only when
the property is absent and requires that exact carrier and direct value before neutral dispatch at
`design.rs:3183` and `design.rs:4569`. Boolean indices `0` through `2` select Join, Cut, and
Intersect. Revolution and Groove indices `0` through `4` select their documented angle,
through-all/last, first, face, and two-angle families.

PartDesign Pad and Pocket register `SideType`, `Type`, and `Type2` as
`App::PropertyEnumeration` values with constructor index `0`. `SideType` indices `0`, `1`, and
`2` select one-sided, two-sided, and symmetric extents. Pad `Type` and `Type2` indices `0`, `1`,
`2`, `3`, and `5` select Length, UpToLast, UpToFirst, UpToFace, and UpToShape. Pocket uses the
same indices for Length, ThroughAll, UpToFirst, UpToFace, and UpToShape. Each present selected
carrier requires one direct `Integer` value; a non-integer, negative, unknown, nested, duplicate,
or wrong-runtime carrier leaves the operation native. An absent `Type` or `Type2` uses Length.
An absent `SideType` uses one side, except that `Midplane=true` selects symmetric legacy
semantics and an exact absent-`SideType` `Type=4` carrier selects the deprecated two-length
two-sided blind form; the `Type=4` form takes precedence over `Midplane=true`. In that form,
`Length` and `Length2` are the two blind extents. A present `Type=4` is not a current termination
family.

Pad and Pocket also persist `Midplane`, `UseCustomVector`, `AlongSketchNormal`, `Reversed`, and
`AllowMultiFace` as `App::PropertyBool` values. `FeaturePad.cpp:45-76` and
`FeaturePocket.cpp:46-84` register the shared carriers and establish `UseCustomVector=false` and
`AlongSketchNormal=true`. `FeatureSketchBased.cpp:72-126` establishes `Midplane=false`,
`Reversed=false`, and `AllowMultiFace=false`; its `setupObject()` changes the last value to true
for a newly created object. `FeatureExtrude.cpp:75-125,435-475` uses `UseCustomVector` to choose
between `Direction`, `ReferenceAxis`, and the profile normal, applies `AlongSketchNormal` to blind
length correction, and applies `Reversed` to the resolved sweep direction. Its
`onDocumentRestored()` at `:963-1013` gives deprecated `Type=?TwoLengths` precedence over
`Midplane`, then maps `Midplane=true` to `SideType=Symmetric`. `FeatureSketchBased.cpp:220-225,310-315`
uses `AllowMultiFace` for profile face admission. `PropertyStandard.cpp:2258-2277` writes and
restores direct Bool values, while `PropertyContainer.cpp:324-404` leaves an omitted property at
the restore-time object value.

The producer witness `dp15_pad_flag_witness.py` saves default and selected Pad/Pocket pairs.
Their extracted `dp15-pad-flags-default.Document.xml` and
`dp15-pad-flags-selected.Document.xml` contain direct false/true values. The
`dp15_pad_flag_mutations.py` absent variants and `dp15_pad_flag_restore_probe.py` show that an
absent Midplane, UseCustomVector, AlongSketchNormal, Reversed, and AllowMultiFace restores as
false, false, true, false, and false for both feature types. `dp15_pad_midplane_conflict.py` and
`dp15-pad-flag-restore-conflict.log` show that a present true Midplane changes an explicit
SideType to Symmetric; the deprecated Type=4 witness retains two-sided precedence. The hostile
batch from `dp15_pad_flag_hostile_mutations.py` has one wrong-runtime, integer-runtime,
invalid-value, nested, and duplicate variant for each flag and retains both Pad and Pocket
natively with two blocking losses per file. Source and absent witnesses have zero decode losses.

`design.rs:3740-3888` now requires an exact direct Bool for every one of these carriers, uses the
restore-time defaults only when a carrier is absent, gives Midplane its traced precedence, and
projects `AlongSketchNormal` and `AllowMultiFace` as explicit neutral booleans. The owner test
`distinguishes_absent_and_malformed_partdesign_extrusion_flags` covers Pad and Pocket, every
absent and valid default, and wrong-runtime, integer-runtime, invalid-value, nested, and duplicate
carriers.

PartDesign `Fillet` declares `UseAllEdges` with a false constructor default at
`FeatureFillet.cpp:53-67` and uses true for every base edge and false for the `Base` edge selection
at `FeatureFillet.cpp:83-98`. PartDesign `Chamfer` declares false-default `FlipDirection` and
`UseAllEdges` at `FeatureChamfer.cpp:68-95`; its executor uses `UseAllEdges` for the edge set and
`FlipDirection` for the chamfer direction at `FeatureChamfer.cpp:128-172`. The Part fillet and
chamfer implementations inherit `Base`, `Edges`, and `EdgeLinks` from `Part::FilletBase` at
`PartFeature.cpp:1978-2037` and do not declare either PartDesign flag. `Part::Scale` declares
`Uniform=true`, `UniformScale=1`, and `XScale`/`YScale`/`ZScale=1` at `FeatureScale.cpp:45-74`;
`computeFinalParameters()` and `scaleShape()` select the uniform or three-axis factors at
`FeatureScale.cpp:85-106`. `PropertyStandard.cpp:2258-2277` writes and restores direct Bool
values, and `PropertyContainer.cpp:324-404` leaves an omitted property at its restore-time
constructor value.

The producer witness `dp15_dress_scale_flag_witness.py` saves default and selected Fillet,
Chamfer, and Scale objects. Extracted `dp15-dress-scale-flags-default.Document.xml` and
`dp15-dress-scale-flags-selected.Document.xml` contain direct Bool values; the selected file has
`UseAllEdges=true`, `FlipDirection=true`, and `Uniform=false` with scale factors `2`, `3`, and
`4`. `dp15_dress_scale_flag_restore_probe.py` and `dp15-dress-scale-flag-restore.log` show that
each omitted carrier restores as false for both dress-up flags and true for `Uniform`. The five
wrong-runtime, integer-runtime, invalid-value, nested, and duplicate mutations for each carrier
are produced by `dp15_dress_scale_flag_hostile_mutations.py`.

`FeatureChamfer.cpp:236-256` adds a version rule: when `ProgramVersion` begins with `0` and
`ChamferType` is `1` or `2`, restore inverts `FlipDirection`; it does not invert index `0`, a
version beginning with `1`, or an absent ProgramVersion. `dp15_chamfer_version_mutations.py`
and `dp15-chamfer-version-restore.log` prove the old two-distance and distance-angle cases,
the equal-distance case, the 1.0 case, and the absent-version case. The rebuilt CLI query of
those files reports the same effective values and zero losses.

The rebuilt CLI dump/check batch in `dp15-dress-scale-cli/` reports zero decode losses for the
default, selected, and every absent-carrier witness. A malformed dress-up carrier falls through
to cached `StoredGeometry` when the producer file has a cached Shape; a malformed Scale carrier
retains `Part::Scale` natively with one blocking loss. The owner tests
`distinguishes_absent_and_malformed_dress_up_flags`,
`distinguishes_absent_and_malformed_part_scale_uniform_flag`, and
`applies_legacy_partdesign_chamfer_flip_migration` cover direct, absent, malformed, and versioned
cases. `design.rs:3883-4008` now scopes these carriers to their producer kinds, applies the
restore defaults only when absent, and applies the old-version chamfer inversion.

Part `Loft` declares `Solid=true`, `Ruled=false`, `Closed=false`, and `Linearize=false` in
`PartFeatures.cpp:177-194`; its executor passes the first three to `makeElementLoft` and applies
linearization when selected at `:224-245`. PartDesign loft declares `Ruled=false` and
`Closed=false` at `FeatureLoft.cpp:45-52`, inherits `AllowMultiFace=false` from
`FeatureSketchBased.cpp:72-110`, and is always built as a solid at `FeatureLoft.cpp:211-230`.
`ProfileBased::setupObject()` changes `AllowMultiFace` only for a newly created object at
`FeatureSketchBased.cpp:123-126`; `PropertyContainer.cpp:324-404` leaves an omitted carrier at
the constructor value during restore. `PropertyStandard.cpp:2258-2277` writes and restores the
direct Bool roots. PartDesign loft does not declare `Linearize`.
The installed producer's runtime property inventory lists no `CheckCompatibility` property for
either `Part::Loft` or `PartDesign::AdditiveLoft`; its saved `Document.xml` likewise contains no
such property. The complete producer-source search finds only `CheckCompatibility` calls on
OpenCASCADE loft builders in `TopoShape.cpp:2579,2650` and `TopoShapeExpansion.cpp:4574`, not a
persistent Loft carrier.

Part `Sweep` declares `Solid=true`, `Frenet=true`, `Transition=1`, and `Linearize=false` at
`PartFeatures.cpp:256-275`; its executor consumes the two booleans and applies linearization at
`:331-347`. PartDesign `Pipe` declares `SpineTangent=false`, `AuxiliarySpineTangent=false`,
`AuxiliaryCurvilinear=true`, and `AllowMultiFace=false` at `FeaturePipe.cpp:61-107`; its
orientation executor consumes `AuxiliaryCurvilinear` for auxiliary mode at `:638-655`, and the
profile path carries the primary and auxiliary selectors at `FeaturePipe.cpp:675-730`. The
current source declares the tangent flags and preserves them in the document, while the
continuous-edge call remains disabled in `buildPipePath`; the neutral record retains their
stored values without inventing additional path selectors.

The producer witness `dp15_loft_sweep_flag_witness.py` records the default and selected values in
`dp15-loft-sweep-flag-witness.log`; extracted `dp15-loft-sweep-flags-default.Document.xml` and
`dp15-loft-sweep-flags-selected.Document.xml` contain the direct Bool roots and selected values.
`dp15_loft_sweep_flag_restore_probe.py` removes all target carriers and records constructor
defaults in `dp15-loft-sweep-flag-restore.log`. The hostile batch from
`dp15_loft_sweep_flag_hostile_mutations.py` covers wrong-runtime, integer-runtime, invalid,
nested, and duplicate direct-root forms for every settled carrier. After rebuilding the CLI,
every source, selected, and absent file dumps and checks successfully. Every malformed PartDesign
pipe and standalone sweep reports one blocking native loss; malformed lofts with cached shapes
use `StoredGeometry` and report no loss. The owner test
`distinguishes_absent_and_malformed_loft_sweep_boolean_flags` covers the same absent, valid, and
malformed admissions without cached shapes.

The standalone Part loft `Linearize` carrier is now represented by the Loft neutral
`linearize` Boolean. The producer witness `dp15-loft-sweep-flags-default.Document.xml` contains
`Linearize=false`, and `dp15-loft-sweep-flags-selected.Document.xml` contains `Linearize=true`;
`dp15-loft-sweep-flag-restore.log` records `Linearize=false` after the carrier is removed. The
owner test also admits an exact direct true carrier, preserves false for an absent carrier and a
same-named PartDesign carrier, and retains a wrong-runtime Part carrier natively.

Part::Extrusion `DirMode` is an `App::PropertyEnumeration` with constructor default `0`.
Indices `0`, `1`, and `2` mean Custom, Edge, and Normal; they select `Dir`, `DirLink`, and the
base-shape normal respectively. A present selected carrier requires one direct `Integer` value;
a non-integer, negative, unknown, nested, duplicate, or wrong-runtime carrier leaves the
extrusion native. An absent `DirMode` selects Custom.

`FeatureExtrusion.cpp:131-151,215-278` registers `Solid`, `Reversed`, and `Symmetric` with
constructor defaults false and applies them to the solid result, direction, and symmetric length
calculation. `PropertyStandard.cpp:2258-2277` writes and restores each Bool carrier. The producer
witness `dp15_extrusion_flags_witness.py` saves false/false/false and true/true/true objects with
non-null shapes; `dp15_extrusion_flags_restore_probe.py` restores false for all three after their
removal. Rebuilt CLI source and absent checks report zero losses. The hostile batch reports one
blocking native loss for every wrong-runtime, integer-runtime, malformed, nested, duplicate, and
unknown carrier for each flag.

Part `Revolution` declares `Symmetric` and `Solid` as Bool properties with false constructor
defaults at `FeatureRevolution.cpp:78-92`. PartDesign `Revolution` and `Groove` inherit
`Midplane`, `Reversed`, and `AllowMultiFace` from `FeatureSketchBased.cpp:72-126`; their
revolution and groove constructors establish the shared `Revolved` base at
`FeatureRevolution.cpp:41-50` and `FeatureGroove.cpp:34-46`. `FeatureRevolved.cpp:163-188,230-238`
applies reversal and midplane behavior, and `FeatureSketchBased.cpp:220-225,310-315` applies
the multi-face choice. `PropertyStandard.cpp:2258-2277` writes and restores direct Bool values.
The producer witness `dp15_revolution_flags_witness.py` saves selected Part and PartDesign
flags; its extracted `dp15-revolution-flags-source.Document.xml` contains the direct carriers.
`dp15-revolution-flags-restore.log` shows the source values and the all-selected-properties-absent
values. Rebuilt CLI checks report zero losses for the source and absent files; the after summary
and loss-count report show one blocking native loss for every wrong-runtime, integer-runtime,
malformed, nested, duplicate, and non-Boolean-value carrier for all five flags.

`design.rs:1932-1946` now admits only an exact direct Bool with the producer's `true` or `false`
value, and `design.rs:3197-3309` applies the family-specific flags. The owner test
`distinguishes_absent_and_malformed_revolution_flags` covers absent, valid, wrong-runtime,
integer-runtime, invalid-value, nested, and duplicate carriers for Part `Revolution` and
PartDesign `Revolution`; existing Revolution/Groove branch tests cover the shared operation
families.

Part and PartDesign thickness and Part offset `Mode` carriers use indices `0`, `1`, and `2` for
Skin, Pipe, and RectoVerso/BothSides. `Part::Thickness` and `Part::Offset` `Join` carriers use
indices `0`, `1`, and `2` for Arc, Tangent, and Intersection. `PartDesign::Thickness` has only
`Join` indices `0` and `1`, for Arc and Intersection; its source maps index `1` to the kernel's
intersection join because it does not offer tangent joining. `Part::Offset2D` inherits the mode and
join carriers but its constructor changes the absent `Mode` default to index `1` (Pipe), and its
executor rejects index `2` (RectoVerso). `FeatureOffset.cpp:36-51,120-175`,
`PartFeatures.cpp:362-376,423-450`, and `FeatureThickness.cpp:40-60,121-136` establish these
carriers, defaults, labels, and execution paths. `FeatureProjectOnSurface.cpp:53-70,151-186`
establishes `ProjectOnSurface.Mode` indices `0`, `1`, and `2` as All, Faces, and Edges, with
constructor default `0`.

`design.rs:1910-1927` now supplies those defaults only for absent properties and requires an exact
`App::PropertyEnumeration` with one direct `Integer` for a selected carrier. The owner test
`distinguishes_absent_and_malformed_shell_and_surface_selectors` and the producer witness
`dp15_shell_surface_witness.py` cover absent defaults, each selected mode, the PartDesign join
mapping, the Offset2D unsupported mode, and selected malformed carriers.

PartDesign `LinearPatternExtension` registers `Mode` and `Mode2` as enumeration properties with
the direct labels Extent and Spacing and constructor index `0`; its spacing executor uses total
extent for index `0` and explicit, repeating, then offset spacing for index `1`. `PolarPattern`
registers the same `Mode` sequence and default. `LinearPatternExtension.h:60-76`,
`LinearPatternExtension.cpp:47-124,127-192,299-349`, `PolarPatternExtension.h:54-61`, and
`PolarPatternExtension.cpp:48-125` establish the carriers, labels, defaults, and calculations.
The PartDesign linear and polar initializers use those shared extensions. `design.rs:5347` and
`design.rs:5375` now require the exact enumeration carrier for Mode and an active second-direction
Mode2. `dp15_pattern_mode_witness.py` writes both linear modes, both polar modes, and a two-axis
linear pattern; its extracted Document.xml has the corresponding direct indices. The producer
restore probe gives Extent for every absent Mode and active Mode2. The source and absent files
check with zero decode losses. The selected malformed-carrier batches, including the active
two-axis Mode2 batch, retain the pattern as native with one blocking loss per file.

PartDesign `Hole` registers `ThreadType`, `HoleCutType`, `DepthType`, `DrillPoint`,
`ThreadDepthType`, and `ThreadDirection` as `App::PropertyEnumeration` carriers. Their source
sequences are respectively None/ISO metric/ISO metric fine/UNC/UNF/UNEF/NPT/BSP/BSW/BSF/ISO tyre;
None/Counterbore/Countersink/Counterdrill; Dimension/ThroughAll; Flat/Angled; Hole Depth/
Dimension/Tapped (DIN76); and Right/Left. The constructor defaults are indices 0, 0, 0, 1, 0,
and 0. `FeatureHole.h:53-79,150-158,160-197`, `FeatureHole.cpp:76-103,467-535,551-625`,
`PropertyStandard.cpp:397-454`, and `PropertyContainer.cpp:324-404` establish the carriers,
labels, defaults, direct Integer serialization, and absent-property restore. `design.rs:4826,4852,
4859,4878,4903,4908` now requires the exact enumeration
carrier and supplies those defaults only when the property is absent.

The producer witness `/home/pcurve/side2/tmp/freecad-l9/dp15_hole_enum_witness.py` saves a
default hole and a selected hole. Extracted `dp15-hole-enum-source.Document.xml` contains direct
values `0,0,0,1,0,0` for the default and `1,1,1,0,1,1` for the selected hole. FreeCAD's
`dp15_hole_enum_restore_probe.py` restores the source and the all-six-carriers-absent mutation as
None/None/Dimension/Angled/Hole Depth/Right. Rebuilt CLI checks of source and absent files report
status ok with zero losses. The selected malformed-carrier batch reports status ok and one
blocking native loss for every wrong-runtime, integer-runtime, malformed, nested, duplicate,
negative, and unknown variant of every six carrier.

`FeatureHole.h:53-90`, `FeatureHole.cpp:551-655,1604-1618,2226-2261`,
`FeatureSketchBased.cpp:98-126`, `TaskHoleParameters.cpp:194-196,446-450,990-995,1172-1174,1242-1244`,
and `PropertyStandard.cpp:116-124,2258-2277` establish the Hole boolean carriers, constructor and
setup defaults, bit assignments, execution tests, UI-produced bitmask values, and direct Bool and
Integer serialization. The producer witnesses `dp15_hole_flags_witness.py` and
`dp15_hole_base_values_witness.py` save the selected booleans, absent-property source mutation,
and BaseProfileType values 0 through 9 and 99. `dp15_hole_flags_restore_probe.py` restores false for
absent boolean carriers and `6` for absent BaseProfileType. The rebuilt CLI source and absent
checks report zero losses. The hostile boolean and integer-carrier batch reports one blocking
native loss for every malformed present carrier; BaseProfileType values with known bits retain
their bit-selected profile, while zero and values with no known bit remain native.

`CosmeticThread` is versioned. The source snapshot identifies itself as FreeCAD 26.3.0-dev in
`version.json` and declares the property with a true constructor value at
`FeatureHole.h:53-56` and `FeatureHole.cpp:551-562`; its `onChanged` path at
`FeatureHole.cpp:1412-1463` uses the property for cosmetic-versus-modeled thread presentation.
The installed producer reports FreeCAD 1.1.1 Revision 44227, and the headless witness
`dp15_cosmetic_thread_current_witness.py` reports no `CosmeticThread` property on a new
`PartDesign::Hole`; its saved `dp15-cosmetic-thread-current.Document.xml` contains no such
carrier. The property inventory log independently records the absent runtime property.

The format half is therefore settled for the installed producer: it has no `CosmeticThread`
carrier. CADIR decision: retain the optional carrier in the neutral model for files from a
producer that persists it, use false when it is absent, and require the exact direct Bool carrier
and producer `true`/`false` value when it is present. A malformed present carrier retains the
Hole natively. `design.rs:4913` now uses the exact Bool selector, and the owner test
`distinguishes_absent_and_malformed_hole_flags` covers absent, valid, wrong-runtime,
integer-runtime, invalid-value, nested, and duplicate carriers.

Part `Helix` and `Spiral` carrier semantics are settled. `PrimitiveFeature.h:317-364` declares
`LocalCoord` and `Style` as `App::PropertyEnumeration` and `SegmentLength` as
`App::PropertyQuantityConstraint`. `PrimitiveFeature.cpp:819-866,943-963` establishes their
constructor defaults and enum values; `:907-927,994-1009` consumes handedness and subdivision;
`TopoShape.cpp:2446-2465` maps an explicit zero subdivision length to the kernel limit. An absent
Part `Helix` `LocalCoord`, `Style`, and `SegmentLength` restore as `0`, `0`, and `0`; an absent
Part `Spiral` `SegmentLength` restores as `1`. `FeatureHelix.cpp:59-214` declares the PartDesign
`Mode`, booleans, and tolerance carriers and their constructor defaults; `:463-485,539-610`
consumes `Outside`, handedness, reversal, mode, growth, and angle. `FeatureSketchBased.cpp:106-126`
and `PropertyContainer.cpp:343-378` establish the restore-time `AllowMultiFace` result. The
producer witness `dp15_helix_carrier_witness.py`, its extracted direct carrier XML, and
`dp15_helix_restore_probe.py` show the selected carriers and restore defaults. The rebuilt CLI
checks of the default, selected, and absent witnesses report zero losses and zero findings.
`design.rs` now requires the exact present carrier types, applies the restore defaults only when
absent, and retains malformed operations natively. The owner test
`distinguishes_absent_and_malformed_helix_carriers` covers all settled selectors and carriers.

**Need.** Apply the same absence-versus-present validation to the remaining design operation
flags. Trace each producer carrier and preserve its restore-time default only when the property is
absent. Pad, Pocket, Revolution, Groove, loft and sweep booleans, dress-up, Scale,
`CosmeticThread` boolean flags, and helix carriers are settled above. ShapeBinder and the remaining
pattern flags still need their own writer and restore evidence.

**Conflict.** The remaining generic `integer_property` and `bool_property` call sites still
collapse a malformed present carrier with an absent property. Their producer defaults and CADIR
salvage rules may differ by operation family; changing them without tracing the writer can change
neutral semantics or discard a valid legacy default. ShapeBinder and the remaining pattern flags
still require their own writer and restore evidence before the generic fallback sites can change.

**Note.** Partly settled: Boolean/Revolution/Groove `Type`, PartDesign Pad/Pocket
`SideType`/`Type`/`Type2` and `Midplane`/`UseCustomVector`/`AlongSketchNormal`/`Reversed`/
`AllowMultiFace`, Part `DirMode` and `Solid`/`Reversed`/`Symmetric` flags, shell/offset
`Mode` and `Join`, `ProjectOnSurface.Mode`,
and LinearPattern/PolarPattern `Mode` and active `Mode2` are covered by the specification and
exact-carrier decoder rule. Revolution and Groove boolean flags, loft and sweep boolean flags,
dress-up `UseAllEdges` and
`FlipDirection` including its old-version migration, Part Scale `Uniform`, Hole enumeration
modes, Hole boolean flags, `BaseProfileType`, the versioned `CosmeticThread` carrier, and Part and
PartDesign helix carriers are covered; ShapeBinder and the remaining pattern flags stay open.

### DP-16. Sketch placement rotation admission

**Question.** Which rotation admission rule applies to a sketch `Placement` or `AttachmentOffset`
carrier: the settled placement rule, or a stricter sketch-only rule?

**Known.** The specification gives one placement rotation rule for every carrier. `A` is the
representation discriminator, a finite zero-length axis is valid and rotates about the positive Z
axis, and every nonzero finite axis is normalized. `product.rs:999-1096` applies that rule;
`attachment.rs` and `joint.rs` call the same function. `design.rs:1588-1653` keeps a second
rotation decode for sketch `Placement` and `AttachmentOffset`; it now uses the positive Z fallback
for a zero axis and normalizes every nonzero finite axis. `validate_sketch_placement` at
`design.rs:1655-1683` still turns incomplete or invalid components into a malformed-document
refusal. FreeCAD `Base::Rotation::setValue` keeps the positive Z axis for a null axis, and
`Base::Vector3::Normalize` divides by every length that is not zero. FreeCAD's quaternion
constructor normalizes its four components at `freecad/src/Base/Rotation.cpp:73-77,200-207,314-325`.

**Need.** Use one rotation admission rule for every placement carrier, or state the sketch rule and
its producer source in the specification. Show which documents each rule accepts and refuses.

**Conflict.** A sketch `Placement` with `A=1.5707963`, `Ox=0`, `Oy=0`, and `Oz=0` is now admitted
as a quarter turn about the positive Z axis, as the producer and `product.rs` require. An axis such
as `Ox=1e-20` is also normalized by the producer and the decoder. A sketch quaternion with finite
components but non-unit norm is normalized by FreeCAD and `product.rs`, while `design.rs` still
applies the raw components. `design.rs:282-290` discards the error from the same function, so an
object that is not transferred as a sketch can lose an extrusion profile normal in silence.

**Note.** Partly settled by the DP-12 zero-axis closure. The remaining quaternion normalization
and non-sketch caller behavior require a separate proof and design decision.

### DP-17. Design enumeration label selection

**Question.** Which direct `Enum` sequence supplies a design enumeration label such as a hole thread
designation?

**Known.** FreeCAD `PropertyEnumeration::Save` writes one direct `Integer` carrier, a
`CustomEnum="true"` marker for a custom list, and one direct `CustomEnumList` whose `count` equals
the number of its direct `Enum` children. `joint.rs:286-413` enforces that framing for `JointType`.
`enumeration_label` at `design.rs:4637-4648` instead takes the `Enum` at the integer position from
every descendant of the property, with no marker, root, count, or leaf check, and it accepts a
`Value` attribute that the producer does not write. `design.rs:4362-4382` uses the result for the
neutral hole thread designation, class, and fit.

**Need.** Use the same direct-carrier framing for every enumeration label as for `JointType`.
Refuse or retain a property whose framing does not match, instead of selecting by descendant order.

**Conflict.** An `Enum` inside an extra nested wrapper joins the ordinal sequence, so a hole gets a
neutral thread designation that the producer does not give it. An index outside the direct list
leaves the designation absent with no refusal and no loss.

**Note.** New hostile-sweep finding.

## 6. Semantic annotations

### SA-03. Annotation value-root framing and attribute selection

**Question.** Which direct value root and canonical attributes supply each registered annotation
text, scalar, vector, and format property?

**Known.** Registered annotation properties have exact runtime types. Standard property writers
emit one direct root: `String` uses `value`, `PropertyVector` uses `valueX`, `valueY`, and `valueZ`,
`Float` uses `value`, and `StringList` owns its ordered direct `String` children. The persistence
layer retains every descendant as a `ValueRecord`. `annotation.rs:50-55` collects text from every
retained descendant, while `annotation.rs:318-376` selects scalar, vector, and format values by
retained-value count without checking the direct root tag. `annotation.rs:448-473` also accepts
capitalized and generic text attributes that the registered property grammar does not define.

**Need.** Enforce the direct root tag, root cardinality, owned child grammar, and canonical
attributes for every registered annotation carrier before neutral transfer. Reject nested roots,
unexpected descendants, and simultaneous or unsupported attribute spellings.

**Conflict.** A registered scalar property with one nested parseable `Float` and no direct
`Float` root passes `unique_value` and supplies a neutral position. A format `String` with both
`value` and `Value` silently selects `value`, and an annotation text property can collect text from
an unrelated nested descendant. Invalid nesting or attribute spelling can therefore create or
change a neutral annotation without a loss.

**Note.** New hostile-sweep finding.

## 7. TechDraw projection

### DG-05. Drawing scalar attribute spelling

**Question.** Which attribute spelling supplies a registered TechDraw scalar value?

**Known.** The registered application-property grammar and FreeCAD scalar writers use the
lowercase `value` attribute. `drawing.rs:550-607` enforces the direct value-root tag and
cardinality, but `drawing.rs:629-634` accepts `Value` when `value` is absent and selects `value`
when both occur.

**Need.** Enforce the producer's canonical scalar attribute and reject unsupported or duplicate
spellings before transferring position, scale, or rotation.

**Conflict.** `<Float Value="2"/>` supplies a neutral drawing scalar although it is outside the
settled property grammar. `<Float value="1" Value="2"/>` silently selects `1`; deleting one
attribute changes the result from `1` to `2` instead of making the contradictory carrier invalid.

**Note.** New hostile-sweep finding.

### DG-06. Non-page template relationship admission

**Question.** Which drawing runtime types may populate the neutral page-template field?

**Known.** `drawing.rs:32-45` extracts typed `Views` and `Template` carriers only for page
objects, but `drawing.rs:66-70` retains every link-valued property as a relationship. Neutral
transfer at `drawing.rs:175-190` then reads any relationship named `Template` for every registered
drawing record. The specification limits page template carriers to pages and states that other
runtime types do not supply them.

**Need.** Gate neutral `Drawing.template` extraction on the page runtime kind. Retain a non-page
`Template` relationship as native relationship data without interpreting it as page membership.

**Conflict.** A registered non-page view with an `App::PropertyLink` named `Template` targeting a
registered template populates `Drawing.template`, although the source type cannot own a page
template carrier. An unrelated link property therefore changes a neutral page field without a
refusal or loss.

**Note.** New hostile-sweep finding.

## 9. Assembly joints

## 10. Attachment and assembly

## 11. Persistent graph admission
