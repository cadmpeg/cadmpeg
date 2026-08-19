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
