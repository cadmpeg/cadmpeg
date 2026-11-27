// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;
use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_ir::design::construction::{
    kinds, Construction, FrameAxis, PostStep, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{FeatureOperation, LinkState};
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, AngleSector, Contact, DefiningRole, DistanceAxis, EntityDefinition, EntityForm,
    Operand, Planar, RadialMeasure, SketchConstraint, SketchEntity, SketchPlane, SketchRelation,
    SolveState,
};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::selection::{
    Anchor, Ordered, PickTarget, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use super::{offset_steps, OffsetBindings};
use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

/// A Body whose sketch lies on the Body's XY plane, offset by 1 mm along X
/// and flipped. Line 1 runs from (0, 0) to (10, 0), line 2 is a
/// construction line from (10, 0) to (10, 5), and circle 3 has its center
/// at (5, 5).
const BODY_SKETCH: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="App::Origin" name="Origin" id="2"/>
 <Object type="App::Plane" name="XY_Plane" id="3"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="3">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Sketch"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Origin" type="App::PropertyLink"><Link value="Origin"/></Property>
 </Properties></Object>
 <Object name="Origin"><Properties Count="1">
  <Property name="OriginFeatures" type="App::PropertyLinkList"><LinkList count="1"><Link value="XY_Plane"/></LinkList></Property>
 </Properties></Object>
 <Object name="XY_Plane"><Properties Count="1">
  <Property name="Role" type="App::PropertyString"><String value="XY_Plane"/></Property>
 </Properties></Object>
 <Object name="Sketch"><Properties Count="7">
  <Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="XY_Plane" sub=""/></LinkSubList></Property>
  <Property name="MapMode" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="MapReversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AttachmentOffset" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="FullyConstrained" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="3">
   <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="10" EndY="0"/><Construction value="0"/></Geometry>
   <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="0" EndX="10" EndY="5"/><Construction value="1"/></Geometry>
   <Geometry type="Part::GeomCircle"><Circle CenterX="5" CenterY="5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="2"/><Construction value="0"/></Geometry>
  </GeometryList></Property>
  <Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="7">
   <Constrain Name="width" Type="6" First="0" FirstPos="0" Value="10"/>
   <Constrain Name="" Type="7" First="0" FirstPos="2" Value="10"/>
   <Constrain Name="width" Type="11" First="2" FirstPos="0" Value="2"/>
   <Constrain Name="" Type="9" First="0" FirstPos="1" Second="1" SecondPos="2" Value="1.5707963267948966"/>
   <Constrain Name="" Type="1" First="0" FirstPos="2" Second="1" SecondPos="1"/>
   <Constrain Name="" Type="2" First="0" FirstPos="0"/>
   <Constrain Name="" Type="5" First="0" FirstPos="2" Second="1" SecondPos="1" Value="0"/>
  </ConstraintList></Property>
 </Properties></Object>
</ObjectData></Document>"#;

fn decode(document: &str) -> CadIr {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    decoded.ir().clone()
}

fn entity_id(suffix: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("fcstd:design:sketch-entity#Sketch:{suffix}")).unwrap()
}

fn entity<'a>(ir: &'a CadIr, suffix: &str) -> &'a SketchEntity<Planar> {
    let id = entity_id(suffix);
    ir.design
        .sketch_entities
        .iter()
        .find(|entity| entity.id == id)
        .unwrap_or_else(|| panic!("{suffix}: {:?}", ir.design.sketch_entities))
}

fn parameter(ir: &CadIr, index: usize) -> &DesignParameter {
    let id = format!("fcstd:design:parameter#Sketch:constraint:{index}");
    ir.design
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id}"))
}

fn bound(ir: &CadIr, index: usize) -> Dimension<cadmpeg_ir::scalar::Length> {
    Dimension::Parameter(parameter(ir, index).id.clone())
}

fn point(suffix: &str) -> Operand<Planar> {
    Operand::Entity(entity_id(suffix))
}

fn relations(ir: &CadIr) -> Vec<&SketchRelation<Planar>> {
    ir.design
        .sketch_constraints
        .iter()
        .map(|constraint| &constraint.relation)
        .collect()
}

#[test]
fn a_body_sketch_on_the_origin_plane_picks_the_body_principal_plane() {
    let ir = decode(BODY_SKETCH);
    let [sketch] = ir.design.sketches.as_slice() else {
        panic!("{:?}", ir.design.sketches);
    };
    let owner = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id == sketch.owner)
        .unwrap();
    assert_eq!(
        owner.definition.operation,
        FeatureOperation::Sketch(sketch.id.clone())
    );
    assert_eq!(sketch.solve.state, Some(SolveState::NotFull));
    assert_eq!(sketch.solve.status, EvaluationStatus::Current);
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("{:?}", sketch.plane);
    };
    let [input] = plane.inputs.as_slice() else {
        panic!("{:?}", plane.inputs);
    };
    let Some(cadmpeg_ir::design::construction::InputReference::Picks(selection)) = &input.reference
    else {
        panic!("{input:?}");
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("{selection:?}");
    };
    let [picked] = members.as_slice() else {
        panic!("{members:?}");
    };
    let PickTarget::Principal { entity, container } = &picked.target else {
        panic!("{picked:?}");
    };
    assert_eq!(*entity, PrincipalEntity::XyPlane);
    assert_eq!(
        container
            .as_ref()
            .map(cadmpeg_ir::design::ids::ContainerId::as_str),
        Some("fcstd:design:container#Body")
    );
    assert_eq!(
        plane.after,
        [
            PostStep::Flip(Dimension::Literal(true)),
            PostStep::Translate {
                axis: FrameAxis::X,
                distance: Dimension::Literal(cadmpeg_ir::scalar::Length::new(1.0).unwrap()),
            },
        ]
    );
    let mode = plane.mode.as_ref().unwrap();
    assert_eq!(
        mode.method,
        NativeNameValue::Text(cadmpeg_core::nonblank_literal!("FlatFace"))
    );
}

#[test]
fn sketch_entities_keep_construction_and_the_points_constraints_name() {
    let ir = decode(BODY_SKETCH);
    assert_eq!(entity(&ir, "1").construction, Some(false));
    assert_eq!(entity(&ir, "2").construction, Some(true));
    for (suffix, role) in [
        ("1:start", DefiningRole::Start),
        ("1:end", DefiningRole::End),
        ("2:start", DefiningRole::Start),
    ] {
        let point = entity(&ir, suffix);
        assert!(
            point
                .roles
                .iter()
                .any(|entity_role| entity_role.role == role),
            "{suffix}: {point:?}"
        );
    }
    assert!(ir
        .design
        .sketch_entities
        .iter()
        .all(|entity| !entity.id.as_str().contains(":reference-")));
}

#[test]
fn constraints_state_the_stored_form_of_each_freecad_relation() {
    let ir = decode(BODY_SKETCH);
    let line = entity_id("1");
    let [length, coordinate, radius, angle, coincident, horizontal, tangent] = relations(&ir)[..]
    else {
        panic!("{:?}", ir.design.sketch_constraints);
    };
    assert_eq!(
        *length,
        SketchRelation::Distance {
            a: point("1:start"),
            b: point("1:end"),
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: bound(&ir, 1),
        }
    );
    assert_eq!(
        *coordinate,
        SketchRelation::Distance {
            a: Operand::Frame(PrincipalEntity::Origin),
            b: point("1:end"),
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: bound(&ir, 2),
        }
    );
    assert_eq!(
        *radius,
        SketchRelation::Radial {
            of: entity_id("3"),
            measure: RadialMeasure::Radius,
            value: bound(&ir, 3),
        }
    );
    let SketchRelation::Angle {
        between: AngleOperands::Between([first, second]),
        sector,
        ..
    } = angle
    else {
        panic!("{angle:?}");
    };
    assert_eq!(*first, Operand::Entity(line.clone()));
    assert_eq!(*second, Operand::Entity(entity_id("2")));
    assert_eq!(
        *sector,
        Some(AngleSector::Directed {
            second_reversed: true,
            clockwise: false,
        })
    );
    assert_eq!(
        *coincident,
        SketchRelation::Coincident {
            a: point("1:end"),
            b: point("2:start"),
        }
    );
    assert_eq!(
        *horizontal,
        SketchRelation::Horizontal {
            a: Operand::Entity(line),
            b: None,
        }
    );
    assert_eq!(
        *tangent,
        SketchRelation::Tangent {
            a: Operand::Entity(entity_id("1")),
            b: Operand::Entity(entity_id("2")),
            contact: Some(Contact::Ends([entity_id("1:end"), entity_id("2:start")])),
        }
    );
}

#[test]
fn a_repeated_constraint_name_names_the_first_constraint_only() {
    let ir = decode(BODY_SKETCH);
    let name = |index| {
        parameter(&ir, index)
            .name
            .as_ref()
            .map(|name| name.as_str().to_owned())
    };
    assert_eq!(name(1).as_deref(), Some("width"));
    assert_eq!(name(2), None);
    assert_eq!(name(3), None);
    let shadowed = &parameter(&ir, 3).native_options;
    assert_eq!(
        shadowed.get(&cadmpeg_ir::codec_name!("fcstd:constraint_name")),
        Some(&NativeOption::Text(Dimension::Literal(
            cadmpeg_core::nonblank_literal!("width")
        )))
    );
    assert!(parameter(&ir, 1).native_options.is_empty());
    assert!(ir
        .design
        .sketch_constraints
        .iter()
        .all(|constraint: &SketchConstraint<Planar>| constraint.name.is_none()));
}

#[test]
fn a_cadir_write_carries_every_sketch_record() {
    let ir = decode(BODY_SKETCH);
    let (written, omitted) = super::super::tests::cadir_round_trip(&ir);
    let carried = ir
        .design
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str().to_owned())
        .chain(
            ir.design
                .sketch_entities
                .iter()
                .map(|entity| entity.id.as_str().to_owned()),
        )
        .chain(
            ir.design
                .sketch_constraints
                .iter()
                .map(|constraint| constraint.id.as_str().to_owned()),
        )
        .chain(
            ir.design
                .parameters
                .iter()
                .map(|parameter| parameter.id.as_str().to_owned()),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(carried.intersection(&omitted).count(), 0, "{omitted:?}");
    assert_eq!(written.design.sketches, ir.design.sketches);
    assert_eq!(written.design.sketch_entities, ir.design.sketch_entities);
    assert_eq!(
        written.design.sketch_constraints,
        ir.design.sketch_constraints
    );
}

#[test]
fn a_detached_sketch_has_no_input() {
    let document = BODY_SKETCH.replace(r#"<Integer value="5"/>"#, r#"<Integer value="0"/>"#);
    let ir = decode(&document);
    let SketchPlane::Placed(plane) = &ir.design.sketches[0].plane else {
        panic!("{:?}", ir.design.sketches);
    };
    assert!(plane.inputs.is_empty());
    assert!(plane.after.is_empty());
    assert!(plane.mode.is_none());
}

/// The plane of the sketch read from `object`.
fn placement<'a>(ir: &'a CadIr, object: &str) -> &'a Construction<kinds::Frame> {
    let id = format!("fcstd:design:sketch#{object}");
    let sketch = ir
        .design
        .sketches
        .iter()
        .find(|sketch| sketch.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id}: {:?}", ir.design.sketches));
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("{:?}", sketch.plane);
    };
    plane
}

/// The pick targets of each input of the plane of the sketch read from
/// `object`.
fn input_picks(ir: &CadIr, object: &str) -> Vec<Vec<PickTarget>> {
    placement(ir, object)
        .inputs
        .iter()
        .map(|input| {
            let Some(cadmpeg_ir::design::construction::InputReference::Picks(selection)) =
                &input.reference
            else {
                panic!("{input:?}");
            };
            let Picks::Named(members) = &selection.picks else {
                panic!("{selection:?}");
            };
            members
                .as_slice()
                .iter()
                .map(|picked| picked.target.clone())
                .collect()
        })
        .collect()
}

/// The single pick of the plane of the sketch read from `object`.
fn support(ir: &CadIr, object: &str) -> PickTarget {
    let [input] = input_picks(ir, object)
        .try_into()
        .unwrap_or_else(|inputs| panic!("{inputs:?}"));
    let [picked] = input.try_into().unwrap_or_else(|picks| panic!("{picks:?}"));
    picked
}

/// The subject and relation of each input of the plane of the sketch read
/// from `object`.
fn roles(ir: &CadIr, object: &str) -> Vec<Option<(Subject, Relation)>> {
    placement(ir, object)
        .inputs
        .iter()
        .map(|input| {
            input
                .role
                .as_ref()
                .map(|role| (role.subject, role.relation.clone()))
        })
        .collect()
}

/// `BODY_SKETCH` with one more sketch in the Body for each of `attached`:
/// its name, the `<Link>` elements of its support, and its `MapMode` index.
fn attached_sketches(attached: &[(&str, &str, u32)]) -> String {
    let objects = attached
        .iter()
        .enumerate()
        .map(|(index, (name, _, _))| {
            format!(
                "\n <Object type=\"Sketcher::SketchObject\" name=\"{name}\" id=\"{}\"/>",
                index + 5
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let data = attached
        .iter()
        .map(|(name, links, mode)| {
            format!(
                r#"<Object name="{name}"><Properties Count="3">
  <Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="{}">{links}</LinkSubList></Property>
  <Property name="MapMode" type="App::PropertyEnumeration"><Integer value="{mode}"/></Property>
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
 </Properties></Object>
"#,
                links.matches("<Link ").count()
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let group = attached
        .iter()
        .map(|(name, _, _)| format!(r#"<Link value="{name}"/>"#))
        .collect::<Vec<_>>()
        .concat();
    let count = 4 + attached.len();
    BODY_SKETCH
        .replace(
            r#"<Objects Count="4">"#,
            &format!(r#"<Objects Count="{count}">"#),
        )
        .replace(
            r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>"#,
            &format!(r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>{objects}"#),
        )
        .replace(
            r#"<ObjectData Count="4">"#,
            &format!(r#"<ObjectData Count="{count}">"#),
        )
        .replace(
            r#"<LinkList count="1"><Link value="Sketch"/></LinkList>"#,
            &format!(
                r#"<LinkList count="{}"><Link value="Sketch"/>{group}</LinkList>"#,
                1 + attached.len()
            ),
        )
        .replace("</ObjectData>", &format!("{data}</ObjectData>"))
}

/// A document of `objects`, each `(type, name, properties)`.
fn document(objects: &[(&str, &str, String)]) -> String {
    let heads =
        objects
            .iter()
            .enumerate()
            .fold(String::new(), |mut out, (index, (kind, name, _))| {
                write!(
                    out,
                    "\n <Object type=\"{kind}\" name=\"{name}\" id=\"{}\"/>",
                    index + 1
                )
                .unwrap();
                out
            });
    let data = objects
        .iter()
        .fold(String::new(), |mut out, (_, name, properties)| {
            write!(
                out,
                "\n <Object name=\"{name}\"><Properties Count=\"{}\">{properties}</Properties></Object>",
                properties.matches("<Property ").count()
            ).unwrap();
            out
        });
    let count = objects.len();
    format!(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\">\n<Objects Count=\"{count}\">{heads}\n</Objects>\n<ObjectData Count=\"{count}\">{data}\n</ObjectData></Document>"
    )
}

/// The properties of a Body of `members` whose `Tip` is the last.
fn body(members: &[&str]) -> String {
    let links = members.iter().fold(String::new(), |mut out, member| {
        write!(out, r#"<Link value="{member}"/>"#).unwrap();
        out
    });
    format!(
        r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="{}">{links}</LinkList></Property><Property name="Tip" type="App::PropertyLink"><Link value="{}"/></Property>"#,
        members.len(),
        members.last().unwrap()
    )
}

/// The properties of a sketch attached in `mode` to `support`, the
/// `<Link>` elements of its support, with external geometry linking each of
/// `externals`, an `(object, subelement)`.
fn sketch_on(support: &str, mode: u32, externals: &[(&str, &str)]) -> String {
    let mut properties = format!(
        r#"<Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="{}">{support}</LinkSubList></Property><Property name="MapMode" type="App::PropertyEnumeration"><Integer value="{mode}"/></Property><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>"#,
        support.matches("<Link ").count()
    );
    if !externals.is_empty() {
        let links = externals
            .iter()
            .fold(String::new(), |mut out, (object, sub)| {
                write!(out, r#"<Link obj="{object}" sub="{sub}"/>"#).unwrap();
                out
            });
        let geometry = externals
            .iter()
            .enumerate()
            .fold(String::new(), |mut out, (index, (object, sub))| {
                write!(
                    out,
                    r#"<Geometry type="Part::GeomLineSegment" id="{}" ref="{object}.{sub}"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="{object}.{sub}" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="1" EndY="1"/></Geometry>"#,
                    index + 1
                ).unwrap();
                out
            });
        write!(
            properties,
            r#"<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkSubList count="{}">{links}</LinkSubList></Property><Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="{}"><Geometry type="Part::GeomLineSegment" id="-1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry><Geometry type="Part::GeomLineSegment" id="-2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>{geometry}</GeometryList></Property>"#,
            externals.len(),
            externals.len() + 2
        ).unwrap();
    }
    properties
}

/// A support link to `object`, or to its subelement `sub`.
fn link(object: &str, sub: &str) -> String {
    format!(r#"<Link obj="{object}" sub="{sub}"/>"#)
}

/// The selection of the single support input of the sketch read from
/// `object`.
fn support_selection<'a>(ir: &'a CadIr, object: &str) -> &'a Selection<Ordered> {
    let [input] = placement(ir, object).inputs.as_slice() else {
        panic!("{:?}", placement(ir, object));
    };
    let Some(cadmpeg_ir::design::construction::InputReference::Picks(selection)) = &input.reference
    else {
        panic!("{input:?}");
    };
    selection
}

fn support_anchor(ir: &CadIr, object: &str) -> Anchor {
    support_selection(ir, object).at.clone()
}

fn source_anchor(ir: &CadIr, object: &str, index: usize) -> Anchor {
    source_selection(ir, object, index).at.clone()
}

/// The source of the external geometry `:external:index` of the sketch
/// read from `object`.
fn source_selection<'a>(ir: &'a CadIr, object: &str, index: usize) -> &'a Selection<Ordered> {
    let suffix = format!("#{object}:external:{index}");
    let entity = ir
        .design
        .sketch_entities
        .iter()
        .find(|entity| entity.id.as_str().ends_with(&suffix))
        .unwrap_or_else(|| panic!("{suffix}: {:?}", ir.design.sketch_entities));
    let EntityDefinition::Defined { form, .. } = &entity.definition else {
        panic!("{entity:?}");
    };
    let EntityForm::Derived(derivation) = form.as_ref() else {
        panic!("{entity:?}");
    };
    &derivation.source
}

fn after(object: &str) -> Anchor {
    Anchor::Pinned(Position::After(Member::Feature(
        cadmpeg_ir::features::FeatureId::mint(format!("fcstd:design:feature#{object}")).unwrap(),
    )))
}

#[test]
fn a_body_sketch_attached_to_an_earlier_solid_feature_reads_the_state_after_it() {
    // `FreeCAD` reads the Pad's shape, the Body's state after the Pad, not
    // the state entering the sketch, which a Pocket between them changes.
    let ir = decode(&document(&[
        (
            "PartDesign::Body",
            "Body",
            body(&["Pad", "Pocket", "Sketch"]),
        ),
        ("PartDesign::Pad", "Pad", String::new()),
        ("PartDesign::Pocket", "Pocket", String::new()),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Pad", "Face1"), 5, &[]),
        ),
    ]));
    assert_eq!(support_anchor(&ir, "Sketch"), after("Pad"));
}

#[test]
fn a_body_sketch_reads_an_earlier_stateless_member_at_its_input_and_a_later_one_natively() {
    // A sketch has no state of its own (§5.4). The Body lists `Later` after
    // the sketch, and `FreeCAD` computes it first; every state that holds
    // `Later` holds the sketch, so no design pick restates it.
    let ir = decode(&document(&[
        (
            "PartDesign::Body",
            "Body",
            body(&["Base", "Sketch", "Later"]),
        ),
        ("Sketcher::SketchObject", "Base", sketch_on("", 0, &[])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Later", ""), 5, &[("Base", "Edge1")]),
        ),
        ("Sketcher::SketchObject", "Later", sketch_on("", 0, &[])),
    ]));
    assert_eq!(
        native_paths(&support_selection(&ir, "Sketch").picks),
        ["Later"]
    );
    assert_eq!(support_anchor(&ir, "Sketch"), Anchor::Input);
    assert_eq!(source_anchor(&ir, "Sketch", 0), Anchor::Input);
}

#[test]
fn a_body_sketch_attached_to_a_later_solid_feature_picks_it_by_native_name() {
    let document = document(&[
        ("PartDesign::Body", "Body", body(&["Sketch", "Pad"])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Pad", "Face1"), 5, &[]),
        ),
        ("PartDesign::Pad", "Pad", String::new()),
    ]);
    let ir = decode(&document);
    assert_eq!(
        native_paths(&support_selection(&ir, "Sketch").picks),
        ["Pad.Face1"]
    );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchSupportUnread,
        "its support is the native pick of each reference"
    ));
}

#[test]
fn a_sketch_attached_into_another_body_reads_the_state_after_its_target() {
    // As the spec reads a binder's source, a feature of another Body is
    // read after itself, stateless or not.
    let ir = decode(&document(&[
        ("PartDesign::Body", "Body", body(&["Base", "Pad"])),
        ("Sketcher::SketchObject", "Base", sketch_on("", 0, &[])),
        ("PartDesign::Pad", "Pad", String::new()),
        ("PartDesign::Body", "Body001", body(&["Sketch"])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Pad", "Face1"), 5, &[("Base", "Edge1")]),
        ),
    ]));
    assert_eq!(support_anchor(&ir, "Sketch"), after("Pad"));
    assert_eq!(source_anchor(&ir, "Sketch", 0), after("Base"));
}

#[test]
fn a_body_sketch_attached_to_a_group_member_reads_its_result() {
    // A member of the `Group` with a shape of its own is read as that
    // result (§5.3); a stateless one has none, and no state holds it.
    let ir = decode(&document(&[
        ("PartDesign::Body", "Body", body(&["Sketch"])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Block", "Face1"), 5, &[("Loose", "Edge1")]),
        ),
        ("Part::Box", "Block", String::new()),
        ("Sketcher::SketchObject", "Loose", sketch_on("", 0, &[])),
    ]));
    assert_eq!(support_anchor(&ir, "Sketch"), after("Block"));
    assert_eq!(
        native_paths(&source_selection(&ir, "Sketch", 0).picks),
        ["Loose.Edge1"]
    );
}

#[test]
fn a_group_sketch_reads_the_result_of_the_one_group_member_it_picks() {
    // §5.8: an operand that picks only the result of another member `m` of
    // the same unordered container is anchored after `m`; a stateless
    // member, or a member of a Body, is read in the current geometry.
    let ir = decode(&document(&[
        ("PartDesign::Body", "Body", body(&["Pad"])),
        ("PartDesign::Pad", "Pad", String::new()),
        ("Part::Box", "Block", String::new()),
        ("Sketcher::SketchObject", "Loose", sketch_on("", 0, &[])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(
                &link("Block", "Face1"),
                5,
                &[("Loose", "Edge1"), ("Pad", "Edge1")],
            ),
        ),
    ]));
    assert_eq!(support_anchor(&ir, "Sketch"), after("Block"));
    assert_eq!(source_anchor(&ir, "Sketch", 0), Anchor::Current);
    assert_eq!(source_anchor(&ir, "Sketch", 1), Anchor::Current);
}

#[test]
fn a_deactivated_attachment_decides_no_anchor() {
    // The support of a deactivated sketch states no input, so it does not
    // pin the external geometry to the state after the Pad.
    let ir = decode(&document(&[
        ("PartDesign::Body", "Body", body(&["Base", "Sketch", "Pad"])),
        ("Sketcher::SketchObject", "Base", sketch_on("", 0, &[])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Pad", "Face1"), 0, &[("Base", "Edge1")]),
        ),
        ("PartDesign::Pad", "Pad", String::new()),
    ]));
    assert!(placement(&ir, "Sketch").inputs.is_empty());
    assert_eq!(source_anchor(&ir, "Sketch", 0), Anchor::Input);
}

#[test]
fn sketches_attached_to_each_other_keep_their_native_definitions() {
    // The decoder keeps each object of a dependency cycle as its native
    // definition, so neither is a sketch record whose picks could close the
    // cycle; the validator's Warning for a cycle of picks is the design
    // layer's own (§5.3).
    let ir = decode(&document(&[
        (
            "Sketcher::SketchObject",
            "First",
            sketch_on(&link("Second", ""), 5, &[]),
        ),
        (
            "Sketcher::SketchObject",
            "Second",
            sketch_on(&link("First", ""), 5, &[]),
        ),
    ]));
    assert!(ir.design.sketches.is_empty(), "{:?}", ir.design.sketches);
}

#[test]
fn a_sketch_attached_to_an_app_link_picks_it_by_native_name() {
    // An `App::Link` places an occurrence and has no result of its own
    // (§5.3), so no design pick restates it.
    let document = document(&[
        ("PartDesign::Body", "Body", body(&["Sketch"])),
        (
            "Sketcher::SketchObject",
            "Sketch",
            sketch_on(&link("Instance", "Face1"), 5, &[("Instance", "Edge1")]),
        ),
        ("Part::Feature", "Prototype", String::new()),
        (
            "App::Link",
            "Instance",
            r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>"#.to_owned(),
        ),
    ]);
    let ir = decode(&document);
    let [input] = placement(&ir, "Sketch").inputs.as_slice() else {
        panic!("{:?}", placement(&ir, "Sketch"));
    };
    let Some(cadmpeg_ir::design::construction::InputReference::Picks(selection)) = &input.reference
    else {
        panic!("{input:?}");
    };
    assert_eq!(native_paths(&selection.picks), ["Instance.Face1"]);
    assert_eq!(source_anchor(&ir, "Sketch", 0), Anchor::Input);
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchSupportUnread,
        "its support is the native pick of each reference"
    ));
}

#[test]
fn a_sketch_attached_to_a_sketch_picks_a_plane_of_its_frame() {
    let attached = |name: &str, mode: &str| {
        format!(
            r#"<Object name="{name}"><Properties Count="3">
  <Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Sketch" sub=""/></LinkSubList></Property>
  <Property name="MapMode" type="App::PropertyEnumeration"><Integer value="{mode}"/></Property>
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
 </Properties></Object>
"#
        )
    };
    let document = BODY_SKETCH
        .replace(
            r#"<Objects Count="4">"#,
            r#"<Objects Count="7">"#,
        )
        .replace(
            r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>"#,
            r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>
 <Object type="Sketcher::SketchObject" name="OnXz" id="5"/>
 <Object type="Sketcher::SketchObject" name="OnXy" id="6"/>
 <Object type="Sketcher::SketchObject" name="OnFace" id="7"/>"#,
        )
        .replace(r#"<ObjectData Count="4">"#, r#"<ObjectData Count="7">"#)
        .replace(
            r#"<LinkList count="1"><Link value="Sketch"/></LinkList>"#,
            r#"<LinkList count="4"><Link value="Sketch"/><Link value="OnXz"/><Link value="OnXy"/><Link value="OnFace"/></LinkList>"#,
        )
        .replace(
            "</ObjectData>",
            &format!(
                "{}{}{}</ObjectData>",
                attached("OnXz", "3"),
                attached("OnXy", "2"),
                attached("OnFace", "5"),
            ),
        );
    let ir = decode(&document);
    let sketch = cadmpeg_ir::sketches::SketchId::mint("fcstd:design:sketch#Sketch").unwrap();
    for (object, entity) in [
        ("OnXz", PrincipalEntity::XzPlane),
        ("OnXy", PrincipalEntity::XyPlane),
        ("OnFace", PrincipalEntity::XyPlane),
    ] {
        assert_eq!(
            support(&ir, object),
            PickTarget::SketchFrame {
                sketch: sketch.clone(),
                entity
            },
            "{object}"
        );
    }
}

#[test]
fn an_external_geometry_of_an_unread_object_picks_its_link_by_native_name() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="Part::FeaturePython" name="Window"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Window" sub="Edge1"/></LinkSubList></Property>
<Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="3">
<Geometry type="Part::GeomLineSegment" id="-1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="-2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="1" ref="Window.Edge1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="Window.Edge1" Flags="0"/></GeoExtensions><LineSegment StartX="2" StartY="3" EndX="4" EndY="3"/></Geometry>
</GeometryList></Property>
</Properties></Object><Object name="Window"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    let entity = ir
        .design
        .sketch_entities
        .iter()
        .find(|entity| entity.id.as_str().ends_with(":external:0"))
        .unwrap_or_else(|| panic!("{:?}", ir.design.sketch_entities));
    let EntityDefinition::Defined { form, .. } = &entity.definition else {
        panic!("{entity:?}");
    };
    let EntityForm::Derived(derivation) = form.as_ref() else {
        panic!("{entity:?}");
    };
    assert_eq!(
        native_paths(&derivation.source.picks),
        ["Window.Edge1"],
        "{derivation:?}"
    );
    let unread = crate::loss::FreecadLossCode::DesignSketchSourceUnread.note("");
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == unread.code && loss.message.contains(":external:0")),
        "{:?}",
        decoded.report().losses
    );
}

/// The `FreeCAD` paths of `picks`, each a `Native` pick that no design
/// record holds.
fn native_paths(picks: &Picks<Ordered>) -> Vec<String> {
    let Picks::Named(members) = picks else {
        panic!("{picks:?}");
    };
    members
        .as_slice()
        .iter()
        .map(|picked| {
            assert!(
                matches!(picked.target, PickTarget::Native { within: None }),
                "{picked:?}"
            );
            match picked.native.as_ref().map(|native| &native.value) {
                Some(NativeNameValue::Text(path)) => path.as_str().to_owned(),
                other => panic!("{other:?}"),
            }
        })
        .collect()
}

#[test]
fn an_external_geometry_with_no_stored_curve_picks_its_link_by_native_name() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="Part::FeaturePython" name="Window"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="Window" sub="Edge4"/><Link obj="Window" sub="Edge2"/></LinkSubList></Property>
</Properties></Object><Object name="Window"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    let sources = ir
        .design
        .sketch_entities
        .iter()
        .filter(|entity| entity.id.as_str().contains(":external:"))
        .map(|entity| {
            let EntityDefinition::Defined { form, solved } = &entity.definition else {
                panic!("{entity:?}");
            };
            let EntityForm::Derived(derivation) = form.as_ref() else {
                panic!("{entity:?}");
            };
            assert!(solved.is_none(), "{entity:?}");
            native_paths(&derivation.source.picks)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        [
            vec!["Window.Edge4".to_owned()],
            vec!["Window.Edge2".to_owned()]
        ],
        "{:?}",
        ir.design.sketch_entities
    );
}

#[test]
fn an_external_geometry_is_a_projection_of_its_linked_subelement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="Part::Feature" name="Source"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="Source" sub="Edge1"/><Link obj="Source" sub="Edge2"/></LinkSubList></Property>
<Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="5">
<Geometry type="Part::GeomLineSegment" id="-1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="-2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>
<Geometry type="Part::GeomCircle" id="1" ref="Source.Edge2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="Source.Edge2" Flags="0"/></GeoExtensions><Circle CenterX="2" CenterY="3" Radius="1"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="2" ref="Source.Edge2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="Source.Edge2" Flags="0"/></GeoExtensions><LineSegment StartX="2" StartY="3" EndX="4" EndY="3"/></Geometry>
<Geometry type="Part::GeomPoint" id="3" ref="Source.Edge1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="Source.Edge1" Flags="3"/></GeoExtensions><GeomPoint X="5" Y="6" Z="0"/></Geometry>
</GeometryList></Property>
</Properties></Object><Object name="Source"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let ir = decode(document);
    let derived = |suffix: &str| {
        let entity = ir
            .design
            .sketch_entities
            .iter()
            .find(|entity| entity.id.as_str().ends_with(suffix))
            .unwrap_or_else(|| panic!("{suffix}: {:?}", ir.design.sketch_entities));
        let EntityDefinition::Defined { form, solved } = &entity.definition else {
            panic!("{entity:?}");
        };
        let EntityForm::Derived(derivation) = form.as_ref() else {
            panic!("{entity:?}");
        };
        assert!(solved.is_some(), "{entity:?}");
        let Picks::Named(members) = &derivation.source.picks else {
            panic!("{derivation:?}");
        };
        let [picked] = members.as_slice() else {
            panic!("{members:?}");
        };
        assert!(
            matches!(picked.target, PickTarget::Native { within: Some(_) }),
            "{picked:?}"
        );
        let Some(NativeNameValue::Text(sub)) = picked.native.as_ref().map(|name| &name.value)
        else {
            panic!("{picked:?}");
        };
        (
            sub.as_str().to_owned(),
            derivation.piece,
            derivation.link,
            entity.construction,
        )
    };
    assert_eq!(
        derived(":external:0"),
        (
            "Edge2".to_owned(),
            Some(0),
            Some(LinkState::Current),
            Some(true)
        )
    );
    assert_eq!(
        derived(":external:1"),
        (
            "Edge2".to_owned(),
            Some(1),
            Some(LinkState::Current),
            Some(true)
        )
    );
    assert_eq!(
        derived(":external:2"),
        (
            "Edge1".to_owned(),
            None,
            Some(LinkState::Locked),
            Some(false)
        )
    );
}

#[test]
fn an_offset_rotation_is_a_yaw_about_z() {
    let rows = [
        [0.0, -1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let steps = offset_steps(rows, &OffsetBindings::default());
    let [PostStep::Rotate {
        axis: FrameAxis::Z,
        angle: Dimension::Literal(angle),
    }] = steps[..]
    else {
        panic!("{steps:?}");
    };
    assert!((angle.get() - FRAC_PI_2).abs() < 1e-12);
}

/// Whether decoding `document` charges `code` with a message that holds
/// `subject`.
fn charges(document: &str, code: crate::loss::FreecadLossCode, subject: &str) -> bool {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let code = code.note("").code;
    decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code && loss.message.contains(subject))
}

#[test]
fn a_sketch_on_an_object_no_design_record_restates_has_an_unread_support() {
    let document = BODY_SKETCH
        .replace(r#"<Objects Count="4">"#, r#"<Objects Count="5">"#)
        .replace(
            r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>"#,
            r#" <Object type="Sketcher::SketchObject" name="Sketch" id="4"/>
 <Object type="Part::FeaturePython" name="Wall" id="5"/>"#,
        )
        .replace(r#"<ObjectData Count="4">"#, r#"<ObjectData Count="5">"#)
        .replace(
            r#"<Link obj="XY_Plane" sub=""/>"#,
            r#"<Link obj="Wall" sub=""/>"#,
        )
        .replace(
            "</ObjectData>",
            r#"<Object name="Wall"><Properties Count="0"/></Object></ObjectData>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchSupportUnread,
        "fcstd:design:sketch#Sketch"
    ));
}

#[test]
fn an_offset_of_a_translate_attachment_is_left_to_the_resolved_frame() {
    let document = BODY_SKETCH.replace(r#"<Integer value="5"/>"#, r#"<Integer value="1"/>"#);
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchOffsetUnstated,
        "fcstd:design:sketch#Sketch"
    ));
    let ir = decode(&document);
    let SketchPlane::Placed(plane) = &ir.design.sketches[0].plane else {
        panic!("{:?}", ir.design.sketches);
    };
    assert!(plane.after.is_empty(), "{:?}", plane.after);
}

#[test]
fn a_curve_with_no_planar_form_states_its_native_kind() {
    let document = BODY_SKETCH
        .replace(r#"<GeometryList count="3">"#, r#"<GeometryList count="4">"#)
        .replace(
            "</GeometryList>",
            r#"<Geometry type="Part::GeomOffsetCurve"><OffsetCurve Offset="1"/><Construction value="0"/></Geometry></GeometryList>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchEntityNative,
        "fcstd:design:sketch-entity#Sketch:4"
    ));
    let ir = decode(&document);
    assert!(
        matches!(
            &entity(&ir, "4").definition,
            EntityDefinition::Defined { form, .. } if matches!(form.as_ref(), EntityForm::Native(_))
        ),
        "{:?}",
        entity(&ir, "4")
    );
}

#[test]
fn a_constraint_form_no_typed_relation_holds_stays_native() {
    let document = BODY_SKETCH
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="" Type="4" First="0" FirstPos="1" Second="1" SecondPos="2"/></ConstraintList>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchRelationNative,
        "fcstd:design:sketch-constraint#Sketch:8"
    ));
    let ir = decode(&document);
    assert!(
        relations(&ir)
            .iter()
            .any(|relation| matches!(relation, SketchRelation::Native { kind, .. } if kind.as_str() == "fcstd:parallel")),
        "{:?}",
        relations(&ir)
    );
}

#[test]
fn a_constraint_on_a_point_the_curve_does_not_store_states_the_curve() {
    let document = BODY_SKETCH
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="" Type="1" First="2" FirstPos="1" Second="0" SecondPos="1"/></ConstraintList>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchLocusUnplaced,
        "fcstd:design:sketch-constraint#Sketch:8"
    ));
}

#[test]
fn a_sketch_attached_by_the_shape_of_a_sketch_picks_the_sketch_with_no_role() {
    let whole = r#"<Link obj="Sketch" sub=""/>"#;
    let document = attached_sketches(&[("Inertial", whole, 45), ("Normal", whole, 7)]);
    let ir = decode(&document);
    let sketch = cadmpeg_ir::sketches::SketchId::mint("fcstd:design:sketch#Sketch").unwrap();
    for (object, mode) in [("Inertial", "InertialCS"), ("Normal", "NormalToEdge")] {
        assert_eq!(roles(&ir, object), [None], "{object}");
        assert_eq!(support(&ir, object), PickTarget::Sketch(sketch.clone()));
        assert!(
            charges(
                &document,
                crate::loss::FreecadLossCode::DesignSketchSupportUnread,
                &format!("fcstd:design:sketch#{object} is attached in mode {mode}")
            ),
            "{object}"
        );
    }
}

#[test]
fn parallel_plane_and_folding_attachments_state_their_relations() {
    let document = attached_sketches(&[
        (
            "Parallel",
            r#"<Link obj="Sketch" sub=""/><Link obj="Sketch" sub="Vertex1"/>"#,
            53,
        ),
        (
            "Folded",
            r#"<Link obj="Sketch" sub="Edge1"/><Link obj="Sketch" sub="Edge2"/><Link obj="Sketch" sub="Edge3"/><Link obj="Sketch" sub="Edge4"/>"#,
            15,
        ),
    ]);
    let ir = decode(&document);
    assert_eq!(
        roles(&ir, "Parallel"),
        [
            Some((Subject::Whole, Relation::Parallel)),
            Some((Subject::Whole, Relation::Coincident)),
        ]
    );
    let sketch = cadmpeg_ir::sketches::SketchId::mint("fcstd:design:sketch#Sketch").unwrap();
    let picks = input_picks(&ir, "Parallel");
    assert_eq!(
        picks[0],
        [PickTarget::SketchFrame {
            sketch,
            entity: PrincipalEntity::XyPlane
        }]
    );
    assert!(
        matches!(picks[1][..], [PickTarget::Native { .. }]),
        "{picks:?}"
    );
    assert_eq!(
        roles(&ir, "Folded"),
        [Some((Subject::Whole, Relation::Fold))]
    );
    assert_eq!(input_picks(&ir, "Folded")[0].len(), 4);
}

#[test]
fn an_attachment_mode_with_no_support_is_a_source_setting() {
    let document = BODY_SKETCH.replace(
        r#"<LinkSubList count="1"><Link obj="XY_Plane" sub=""/></LinkSubList>"#,
        r#"<LinkSubList count="0"/>"#,
    );
    let ir = decode(&document);
    let sketch = &ir.design.sketches[0];
    assert!(placement(&ir, "Sketch").inputs.is_empty());
    assert_eq!(
        sketch
            .source_settings
            .get(&cadmpeg_ir::codec_name!("fcstd:MapMode")),
        Some(&NativeOption::Text(Dimension::Literal(
            cadmpeg_core::nonblank_literal!("FlatFace")
        )))
    );
}

#[test]
fn an_offset_component_an_expression_binds_binds_its_step() {
    let document = BODY_SKETCH
        .replace(r#"<Properties Count="7">"#, r#"<Properties Count="8">"#)
        .replace(
            r#"Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>"#,
            r#"Pz="6" Q0="0" Q1="0" Q2="0.25881904510252074" Q3="0.9659258262890683" A="0.5235987755982988" Ox="0" Oy="0" Oz="1"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="2">
   <Expression path=".AttachmentOffset.Base.z" expression="2 * 3"/>
   <Expression path=".AttachmentOffset.Rotation.Angle" expression="30"/>
  </ExpressionEngine></Property>"#,
        );
    let ir = decode(&document);
    let named = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| {
                parameter
                    .name
                    .as_ref()
                    .map(cadmpeg_core::text::NonBlankString::as_str)
                    == Some(name)
            })
            .unwrap_or_else(|| panic!("{name}: {:?}", ir.design.parameters))
            .id
            .clone()
    };
    let (z, angle) = (
        named("AttachmentOffset.Base.z"),
        named("AttachmentOffset.Rotation.Angle"),
    );
    let length = |value: f64| Dimension::Literal(cadmpeg_ir::scalar::Length::new(value).unwrap());
    let zero = || Dimension::Literal(cadmpeg_ir::scalar::Angle::new(0.0).unwrap());
    assert_eq!(
        placement(&ir, "Sketch").after,
        [
            PostStep::Flip(Dimension::Literal(true)),
            PostStep::Translate {
                axis: FrameAxis::X,
                distance: length(1.0),
            },
            PostStep::Translate {
                axis: FrameAxis::Y,
                distance: length(0.0),
            },
            PostStep::Translate {
                axis: FrameAxis::Z,
                distance: Dimension::Parameter(z),
            },
            PostStep::Rotate {
                axis: FrameAxis::Z,
                angle: Dimension::Parameter(angle),
            },
            PostStep::Rotate {
                axis: FrameAxis::Y,
                angle: zero(),
            },
            PostStep::Rotate {
                axis: FrameAxis::X,
                angle: zero(),
            },
        ]
    );
}

#[test]
fn an_offset_axis_an_expression_binds_stays_its_stored_value() {
    let document = BODY_SKETCH
        .replace(r#"<Properties Count="7">"#, r#"<Properties Count="8">"#)
        .replace(
            r#"Q0="0" Q1="0" Q2="0" Q3="1"/></Property>"#,
            r#"Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1">
   <Expression path=".AttachmentOffset.Rotation.Axis.x" expression="0"/>
  </ExpressionEngine></Property>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchOffsetUnbound,
        ".AttachmentOffset.Rotation.Axis.x = 0"
    ));
}

#[test]
fn an_external_geometry_that_stores_no_link_has_an_unread_source() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="3">
<Geometry type="Part::GeomLineSegment" id="-1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="-2"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>
<Geometry type="Part::GeomLineSegment" id="1"><GeoExtensions count="1"><GeoExtension type="Sketcher::ExternalGeometryExtension" Ref="" Flags="0"/></GeoExtensions><LineSegment StartX="2" StartY="3" EndX="4" EndY="3"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    assert!(charges(
        document,
        crate::loss::FreecadLossCode::DesignSketchSourceUnread,
        "stores no link to its source"
    ));
}

#[test]
fn a_deactivated_or_named_internal_alignment_is_a_native_relation() {
    let document = BODY_SKETCH
        .replace(r#"<GeometryList count="3">"#, r#"<GeometryList count="5">"#)
        .replace(
            "</GeometryList>",
            r#"<Geometry type="Part::GeomEllipse"><Ellipse CenterX="20" CenterY="0" MajorRadius="4" MinorRadius="2" MajorAxisX="1" MajorAxisY="0"/><Construction value="0"/></Geometry>
   <Geometry type="Part::GeomLineSegment"><LineSegment StartX="16" StartY="0" EndX="24" EndY="0"/><Construction value="1"/></Geometry>
  </GeometryList>"#,
        )
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="" Type="15" First="4" FirstPos="0" Second="3" SecondPos="0" InternalAlignmentType="1" IsActive="0"/></ConstraintList>"#,
        );
    let ir = decode(&document);
    assert!(
        !entity(&ir, "5")
            .roles
            .iter()
            .any(|role| role.role == DefiningRole::MajorAxis),
        "{:?}",
        entity(&ir, "5")
    );
    let constraint = ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| matches!(&constraint.relation, SketchRelation::Native { kind, .. } if kind.as_str() == "fcstd:internal_alignment"))
        .unwrap_or_else(|| panic!("{:?}", relations(&ir)));
    assert_eq!(constraint.active, Some(Dimension::Literal(false)));
    assert_eq!(
        constraint
            .native_options
            .get(&cadmpeg_ir::codec_name!("fcstd:alignment")),
        Some(&NativeOption::Text(Dimension::Literal(
            cadmpeg_core::nonblank_literal!("EllipseMajorDiameter")
        )))
    );
}

#[test]
fn a_dimension_that_stores_no_value_names_the_missing_value() {
    let document = BODY_SKETCH
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="" Type="6" First="0" FirstPos="1" Second="2" SecondPos="3"/></ConstraintList>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchRelationNative,
        "fcstd:design:sketch-constraint#Sketch:8 stores no readable Value"
    ));
}

#[test]
fn a_named_internal_alignment_is_stated_once() {
    let document = BODY_SKETCH
        .replace(r#"<GeometryList count="3">"#, r#"<GeometryList count="5">"#)
        .replace(
            "</GeometryList>",
            r#"<Geometry type="Part::GeomEllipse"><Ellipse CenterX="20" CenterY="0" MajorRadius="4" MinorRadius="2" MajorAxisX="1" MajorAxisY="0"/><Construction value="0"/></Geometry>
   <Geometry type="Part::GeomLineSegment"><LineSegment StartX="16" StartY="0" EndX="24" EndY="0"/><Construction value="1"/></Geometry>
  </GeometryList>"#,
        )
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="axis" Type="15" First="4" FirstPos="0" Second="3" SecondPos="0" InternalAlignmentType="1"/></ConstraintList>"#,
        );
    let ir = decode(&document);
    assert!(entity(&ir, "5").roles.is_empty(), "{:?}", entity(&ir, "5"));
    let constraint = ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| matches!(&constraint.relation, SketchRelation::Native { kind, .. } if kind.as_str() == "fcstd:internal_alignment"))
        .unwrap_or_else(|| panic!("{:?}", relations(&ir)));
    assert_eq!(constraint.name.as_deref(), Some("axis"));
}

#[test]
fn an_operand_that_names_no_geometry_is_charged() {
    // Type 16 (Snell's law) has no typed relation and is stated natively
    // without a charge; GeoId 9 names no geometry of the sketch.
    let document = BODY_SKETCH
        .replace(r#"<ConstraintList count="7">"#, r#"<ConstraintList count="8">"#)
        .replace(
            "</ConstraintList>",
            r#"<Constrain Name="" Type="16" First="0" FirstPos="2" Second="9" SecondPos="1" Third="1" ThirdPos="0" Value="1.5"/></ConstraintList>"#,
        );
    assert!(charges(
        &document,
        crate::loss::FreecadLossCode::DesignSketchRelationNative,
        "fcstd:design:sketch-constraint#Sketch:8 names GeoId 9 at PointPos 1"
    ));
}
