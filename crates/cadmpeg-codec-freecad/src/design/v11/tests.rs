// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::io::Cursor;

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{FeatureOperation, FuzzyTolerance, OperationFamily};
use cadmpeg_ir::design::ops::assembly::{Place, PlacementBase};
use cadmpeg_ir::design::ops::body::ScaleFactors;
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
use crate::FcstdCodec;

const DESIGN_HISTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
));
const FUZZY_STORED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/named_constraint_1.1.4_N10a.FCStd"
));
const CORE_OPERATIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
));
const BINARY_EXACT_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
));

fn decode(bytes: &[u8], depth: DecodeDepth) -> CadIr {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    decoded.ir().clone()
}

fn root_bodies(ir: &CadIr) -> BTreeSet<String> {
    ir.model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap()
        .bodies
        .iter()
        .map(|body| body.as_str().to_owned())
        .collect()
}

fn feature(id: &str) -> Member {
    Member::Feature(cadmpeg_ir::features::FeatureId::mint(id).unwrap())
}

const GROUP: &str = "fcstd:design:container#document:group";

/// The CADIR write of `ir` read back, and each record id an `arena.omitted`
/// loss of the write names.
fn cadir_round_trip(ir: &CadIr) -> (CadIr, BTreeSet<String>) {
    use cadmpeg_ir::codec::write::{target::TargetRequest, CadirEncoder, EncodeInput, Encoder};
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
        .unwrap();
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "arena.omitted")
        .filter_map(|loss| loss.message.split_once("omitted: "))
        .flat_map(|(_, ids)| ids.split(", ").map(str::to_owned))
        .collect();
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    (written, omitted)
}

/// The members of the scope's root `Group`.
fn group_members(ir: &CadIr) -> BTreeSet<Member> {
    let Some(Container::Unordered {
        kind: UnorderedKind::Group,
        members,
        ..
    }) = ir
        .design
        .containers
        .iter()
        .find(|container| container.id().as_str() == GROUP)
    else {
        panic!("{:?}", ir.design.containers);
    };
    assert!(ir.design.scopes[0]
        .roots
        .iter()
        .any(|root| root.as_str() == GROUP));
    members.clone()
}

#[test]
fn a_partdesign_body_is_an_ordered_body_container_at_its_tip() {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(DESIGN_HISTORY), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let design = &ir.design;
    let [scope] = design.scopes.as_slice() else {
        panic!("one scope: {:?}", design.scopes);
    };
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(scope.definition, root.id);
    let body = design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Body")
        .unwrap();
    let Container::Ordered {
        kind,
        members,
        rollback,
        ..
    } = body
    else {
        panic!("{body:?}");
    };
    assert_eq!(*kind, OrderedKind::Body);
    assert_eq!(
        members.as_slice(),
        [
            "fcstd:design:feature#PadSketch",
            "fcstd:design:feature#Pad",
            "fcstd:design:feature#PocketSketch",
            "fcstd:design:feature#Pocket",
            "fcstd:design:feature#Fillet",
        ]
        .map(feature)
    );
    assert_eq!(*rollback, Some(Rollback::End));
    assert_eq!(scope.roots.as_slice(), [body.id().clone()]);
    assert_eq!(design.containers.len(), 1);
    let [current] = scope.current.as_slice() else {
        panic!("{:?}", scope.current);
    };
    assert_eq!(
        current.at,
        Position::After(feature("fcstd:design:feature#Fillet"))
    );
    assert_eq!(current.bodies, root.bodies);
    assert!(current.bodies[0]
        .as_str()
        .starts_with("fcstd:model:body#Body:Shape:"));
    let sketch = design
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "fcstd:design:feature#PadSketch")
        .unwrap();
    assert_eq!(
        sketch.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
}

#[test]
fn a_tip_before_the_last_member_rolls_the_body_back() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
 <Object type="Part::Feature" name="Second" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let Some(Container::Ordered { rollback, .. }) = ir
        .design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Body")
    else {
        panic!("{:?}", ir.design.containers);
    };
    assert_eq!(
        *rollback,
        Some(Rollback::Before(feature("fcstd:design:feature#Second")))
    );
    assert_eq!(
        ir.design.scopes[0].current[0].at,
        Position::After(feature("fcstd:design:feature#First"))
    );
}

#[test]
fn a_geometry_decode_states_the_root_and_no_design() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(DESIGN_HISTORY),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let ir = decoded.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.features.is_empty());
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.bodies.len(), 1);
}

#[test]
fn an_object_of_a_document_with_no_body_is_a_member_of_the_root_group() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Stored" id="1"/></Objects>
<ObjectData Count="1"><Object name="Stored"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("{:?}", ir.design.scopes);
    };
    assert!(ir
        .model
        .product_definitions
        .iter()
        .any(|definition| definition.is_root() && definition.id == scope.definition));
    assert_eq!(
        scope
            .roots
            .iter()
            .map(cadmpeg_ir::design::ids::ContainerId::as_str)
            .collect::<Vec<_>>(),
        [GROUP]
    );
    assert_eq!(
        group_members(&ir),
        BTreeSet::from([feature("fcstd:design:feature#Stored")])
    );
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Stored { .. }
    ));
    let (written, omitted) = cadir_round_trip(&ir);
    assert!(
        !omitted.contains("fcstd:design:feature#Stored"),
        "{omitted:?}"
    );
    assert_eq!(written.design.features, ir.design.features);
    assert_eq!(written.design.containers, ir.design.containers);
}

#[test]
fn a_body_with_no_member_states_no_scope_and_no_container() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Body" name="Body" id="1"/></Objects>
<ObjectData Count="1"><Object name="Body"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="0"/></Property>
</Properties></Object></ObjectData>
</Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    assert!(ir.design.scopes.is_empty(), "{:?}", ir.design.scopes);
    assert!(ir.design.containers.is_empty());
}

#[test]
fn a_body_freecad_marks_touched_is_stale() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="First"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let stale = decode(&archive(document), DecodeDepth::Design);
    assert_eq!(
        stale.design.scopes[0].current[0].status,
        EvaluationStatus::Stale
    );
    let current = decode(
        &archive(&document.replace(r#" Touched="1""#, "")),
        DecodeDepth::Design,
    );
    assert_eq!(
        current.design.scopes[0].current[0].status,
        EvaluationStatus::Current
    );
}

#[test]
fn a_body_placement_is_its_container_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Body"><Properties Count="3">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="First"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="5" Q0="0" Q1="0" Q2="ROTATION" Q3="ROTATION"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let body_placement = |ir: &CadIr| {
        let Some(Container::Ordered { placement, .. }) = ir
            .design
            .containers
            .iter()
            .find(|container| container.id().as_str() == "fcstd:design:container#Body")
        else {
            panic!("{:?}", ir.design.containers);
        };
        placement.clone()
    };
    // A quarter turn about z: x maps to y, y to -x.
    let turned = decode(
        &archive(&document.replace("ROTATION", &std::f64::consts::FRAC_1_SQRT_2.to_string())),
        DecodeDepth::Design,
    );
    let placement = body_placement(&turned).expect("a stored placement");
    assert!(placement.inputs.is_empty());
    assert!(matches!(
        placement.after.as_slice(),
        [cadmpeg_ir::design::construction::PostStep::Fixed(_)]
    ));
    let frame = placement.resolved.as_ref().expect("a resolved frame");
    let near = |actual: [f64; 3], expected: [f64; 3]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-9)
    };
    let origin = frame.origin.get();
    assert!(near([origin.x, origin.y, origin.z], [10.0, 0.0, 5.0]));
    let axis = |axis: cadmpeg_ir::units::UnitVector3| {
        let axis = cadmpeg_ir::math::Vector3::from(axis);
        [axis.x, axis.y, axis.z]
    };
    assert!(near(axis(frame.x_axis), [0.0, 1.0, 0.0]));
    assert!(near(axis(frame.y_axis), [-1.0, 0.0, 0.0]));
    assert!(near(axis(frame.z_axis), [0.0, 0.0, 1.0]));
    let (written, _) = cadir_round_trip(&turned);
    assert_eq!(body_placement(&written), Some(placement));
    let identity = decode(
        &archive(
            &document
                .replace(r#"Q2="ROTATION" Q3="ROTATION""#, r#"Q2="0" Q3="1""#)
                .replace(r#"Px="10" Py="0" Pz="5""#, r#"Px="0" Py="0" Pz="0""#),
        ),
        DecodeDepth::Design,
    );
    // A stored identity with no binding is absent (§2.4).
    assert_eq!(body_placement(&identity), None);
}

#[test]
fn a_part_feature_states_its_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Compound" name="Compound" id="1"/></Objects>
<ObjectData Count="1"><Object name="Compound"><Properties Count="1">
 <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="PX" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let placement = |ir: &CadIr| {
        ir.design
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "fcstd:design:feature#Compound")
            .unwrap_or_else(|| panic!("{:?}", ir.design.features))
            .placement
            .clone()
    };
    let moved = decode(&archive(&document.replace("PX", "4")), DecodeDepth::Design);
    let stated = placement(&moved).expect("the feature's Placement");
    assert!(stated.inputs.is_empty());
    let origin = stated
        .resolved
        .as_ref()
        .expect("a resolved frame")
        .origin
        .get();
    assert_eq!([origin.x, origin.y, origin.z], [4.0, 0.0, 0.0]);
    let (written, _) = cadir_round_trip(&moved);
    assert_eq!(written.design.features, moved.design.features);
    // A stored identity with no binding is absent (§2.4).
    let identity = decode(&archive(&document.replace("PX", "0")), DecodeDepth::Design);
    assert_eq!(placement(&identity), None);
}

#[test]
fn a_part_section_states_its_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="First" id="1"/>
 <Object type="Part::Box" name="Second" id="2"/>
 <Object type="Part::Section" name="Section" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="First"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Second"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Section"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="First"/></Property>
  <Property name="Tool" type="App::PropertyLink"><Link value="Second"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="3" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let section = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "fcstd:design:feature#Section")
        .unwrap_or_else(|| panic!("{:?}", ir.design.features));
    assert!(matches!(
        section.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Curve
        }
    ));
    // A section applies its `Placement` to its result (§5.10).
    let origin = section
        .placement
        .as_ref()
        .expect("the section's Placement")
        .resolved
        .as_ref()
        .expect("a resolved frame")
        .origin
        .get();
    assert_eq!([origin.x, origin.y, origin.z], [0.0, 3.0, 0.0]);
}

#[test]
fn a_multitransform_stage_is_no_feature() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="Base" id="2"/>
 <Object type="PartDesign::LinearPattern" name="Stage" id="3"/>
 <Object type="PartDesign::MultiTransform" name="Pattern" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Base"/><Link value="Stage"/><Link value="Pattern"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Pattern"/></Property>
 </Properties></Object>
 <Object name="Base"><Properties Count="0"/></Object>
 <Object name="Stage"><Properties Count="0"/></Object>
 <Object name="Pattern"><Properties Count="1">
  <Property name="Transformations" type="App::PropertyLinkList"><LinkList count="1"><Link value="Stage"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first() else {
        panic!("{:?}", ir.design.containers);
    };
    assert_eq!(
        members.as_slice(),
        ["fcstd:design:feature#Base", "fcstd:design:feature#Pattern"].map(feature)
    );
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.id.as_str() != "fcstd:design:feature#Stage"));
}

#[test]
fn refine_and_fuzzy_tolerance_are_stated_where_stored() {
    let pad = |ir: &CadIr| {
        ir.design
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "fcstd:design:feature#Pad")
            .unwrap()
            .definition
            .clone()
    };
    let refine_only = pad(&decode(DESIGN_HISTORY, DecodeDepth::Design));
    assert!(refine_only.refine.is_some());
    assert_eq!(refine_only.fuzzy_tolerance, None);
    let both = pad(&decode(FUZZY_STORED, DecodeDepth::Design));
    assert!(both.refine.is_some());
    assert!(matches!(
        both.fuzzy_tolerance,
        Some(FuzzyTolerance::KernelDefault | FuzzyTolerance::Automatic)
    ));
}

#[test]
fn a_native_kind_uses_the_codec_id() {
    let FeatureOperation::Native { kind, .. } = super::native("PartDesign::FeatureBase") else {
        panic!();
    };
    assert_eq!(kind.as_str(), "fcstd:PartDesign::FeatureBase");
}

#[test]
fn a_parameter_with_an_empty_expression_is_stated_with_its_stored_value() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="PartDesign::Pad" name="Pad" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Pad"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Pad"/></Property>
 </Properties></Object>
 <Object name="Pad"><Properties Count="2">
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="EXPRESSION"/></ExpressionEngine></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="20.0000000000000000"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    for text in ["", "  "] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document.replace("EXPRESSION", text))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ir = decoded.ir();
        assert_valid_document(ir);
        let length = ir
            .design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == "fcstd:design:parameter#Pad:Length")
            .unwrap_or_else(|| panic!("{:?}", ir.design.parameters));
        let wire = json::to_value(&length.definition).unwrap();
        assert_eq!(wire["value"]["kind"], "neutral", "{wire}");
        assert_eq!(wire["value"]["value"]["text"], "20[mm]", "{wire}");
        let empty = crate::loss::FreecadLossCode::ParameterExpressionEmpty.note("");
        assert_eq!(empty.code.to_string(), "fcstd/parameter.expression-empty");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == empty.code
                    && loss.message.contains("fcstd:design:parameter#Pad:Length")),
            "{:?}",
            decoded.report().losses
        );
    }
}

#[test]
fn a_scoped_root_owns_its_current_states_and_unproduced_shapes_at_every_depth() {
    let design = decode(CORE_OPERATIONS, DecodeDepth::Design);
    let [scope] = design.design.scopes.as_slice() else {
        panic!("{:?}", design.design.scopes);
    };
    let current = scope
        .current
        .iter()
        .flat_map(|state| &state.bodies)
        .map(|body| body.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let unproduced = scope
        .unproduced
        .iter()
        .map(|body| body.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert!(!unproduced.is_empty());
    assert!(current.is_disjoint(&unproduced));
    let root = root_bodies(&design);
    assert_eq!(root, &current | &unproduced);
    // A Body member's shape is a state of its Body, which an evaluation
    // states, or the `Tip`'s repeat of the current state: no body of the
    // geometry (§5.4).
    assert!(design
        .geometry
        .bodies
        .iter()
        .all(|body| !body.id.as_str().starts_with("fcstd:model:body#Hole:Shape:")));
    let owners = design
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.bodies)
        .map(|body| body.as_str().to_owned())
        .collect::<Vec<_>>();
    let mut listed = owners.clone();
    listed.sort();
    listed.dedup();
    assert_eq!(listed.len(), owners.len());
    assert!(design
        .geometry
        .bodies
        .iter()
        .all(|body| owners.contains(&body.id.as_str().to_owned())));
    assert_eq!(
        root_bodies(&decode(CORE_OPERATIONS, DecodeDepth::Geometry)),
        root
    );
}

#[test]
fn a_group_member_shape_is_its_current_state() {
    let ir = decode(BINARY_EXACT_SHAPE, DecodeDepth::Design);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("{:?}", ir.design.scopes);
    };
    let carrier = feature("fcstd:design:feature#BinaryCarrier");
    assert_eq!(group_members(&ir), BTreeSet::from([carrier.clone()]));
    let [state] = scope.current.as_slice() else {
        panic!("{:?}", scope.current);
    };
    assert_eq!(state.at, Position::After(carrier));
    let bodies = root_bodies(&ir);
    assert!(!bodies.is_empty());
    assert_eq!(
        state
            .bodies
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<BTreeSet<_>>(),
        bodies
    );
    assert!(scope.unproduced.is_empty(), "{:?}", scope.unproduced);
    let (written, omitted) = cadir_round_trip(&ir);
    assert!(
        !omitted.contains("fcstd:design:feature#BinaryCarrier"),
        "{omitted:?}"
    );
    assert_eq!(written.design.scopes, ir.design.scopes);
}

#[test]
fn objects_outside_a_body_are_group_members_with_their_parameters() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="Part::Feature" name="First" id="2"/>
 <Object type="Part::Box" name="Box" id="3"/>
 <Object type="Spreadsheet::Sheet" name="Sheet" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="First"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="First"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Box"><Properties Count="1">
  <Property name="Length" type="App::PropertyLength"><Float value="10.0000000000000000"/></Property>
 </Properties></Object>
 <Object name="Sheet"><Properties Count="1">
  <Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="0"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("{:?}", ir.design.scopes);
    };
    assert_eq!(scope.roots.len(), 2, "{:?}", scope.roots);
    assert!(ir.design.containers.iter().any(|container| matches!(
        container,
        Container::Ordered { kind: OrderedKind::Body, members, .. }
            if members.as_slice() == [feature("fcstd:design:feature#First")]
    )));
    assert_eq!(
        group_members(&ir),
        BTreeSet::from(["fcstd:design:feature#Box", "fcstd:design:feature#Sheet"].map(feature))
    );
    let operation = |id: &str| {
        ir.design
            .features
            .iter()
            .find(|feature| feature.id.as_str() == id)
            .unwrap_or_else(|| panic!("{:?}", ir.design.features))
            .definition
            .operation
            .clone()
    };
    assert_eq!(
        operation("fcstd:design:feature#Sheet"),
        FeatureOperation::Spreadsheet(
            cadmpeg_ir::spreadsheets::SpreadsheetId::mint("fcstd:design:spreadsheet#Sheet")
                .unwrap()
        )
    );
    let length = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str() == "fcstd:design:parameter#Box:Length")
        .unwrap_or_else(|| panic!("{:?}", ir.design.parameters));
    assert_eq!(
        length.owner,
        Some(ParameterOwner::Feature(
            cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Box").unwrap()
        ))
    );
    let (written, omitted) = cadir_round_trip(&ir);
    for id in [
        "fcstd:design:feature#Box",
        "fcstd:design:feature#Sheet",
        "fcstd:design:parameter#Box:Length",
    ] {
        assert!(!omitted.contains(id), "{id}: {omitted:?}");
    }
    assert_eq!(written.design.features, ir.design.features);
    assert_eq!(written.design.parameters, ir.design.parameters);
}

#[test]
fn a_spreadsheet_is_a_feature_naming_its_sheet_record() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Spreadsheet::Sheet" name="Sheet" id="1"/></Objects>
<ObjectData Count="1">
 <Object name="Sheet"><Properties Count="2">
  <Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="3">
   <Cell address="A1" alias="w" content="12mm"/>
   <Cell address="A2" content="=w * 2"/>
   <Cell address="C1" content="'Title" rowSpan="1" colSpan="2"/>
  </Cells></Property>
  <Property name="columnWidths" type="Spreadsheet::PropertyColumnWidths"><ColumnInfo Count="1"><Column name="C" width="140"/></ColumnInfo></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document), DecodeDepth::Design);
    let [sheet] = ir.design.spreadsheets.as_slice() else {
        panic!("{:?}", ir.design.spreadsheets);
    };
    assert_eq!(sheet.id.as_str(), "fcstd:design:spreadsheet#Sheet");
    let cells = sheet
        .cells()
        .iter()
        .map(|cell| (cell.address.a1(), cell.parameter.as_str().to_owned()))
        .collect::<Vec<_>>();
    let cell = |address: &str| format!("fcstd:design:parameter#Sheet:cell:{address}");
    assert_eq!(
        cells,
        ["A1", "A2", "C1"].map(|address| (address.to_owned(), cell(address)))
    );
    assert_eq!(sheet.column_widths().len(), 1);
    assert_eq!(sheet.column_widths()[0].pixels, 140);
    assert_eq!(sheet.merged_ranges().len(), 1);
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "fcstd:design:feature#Sheet")
        .unwrap();
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::Spreadsheet(sheet.id.clone())
    );
    let (written, omitted) = cadir_round_trip(&ir);
    for id in [
        "fcstd:design:spreadsheet#Sheet".to_owned(),
        cell("A1"),
        cell("A2"),
        cell("C1"),
    ] {
        assert!(!omitted.contains(&id), "{id}: {omitted:?}");
    }
    assert_eq!(written.design.spreadsheets, ir.design.spreadsheets);
    assert_eq!(written.design.parameters, ir.design.parameters);
}

/// The `Place` feature of link `Instance`, a member of the root `Group`.
fn instance_place(ir: &CadIr) -> Place {
    let instance = feature("fcstd:design:feature#Instance");
    assert!(group_members(ir).contains(&instance));
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| Member::Feature(feature.id.clone()) == instance)
        .unwrap_or_else(|| panic!("{:?}", ir.design.features));
    assert_eq!(feature.name.as_deref(), Some("Instance"));
    let FeatureOperation::Place(place) = &feature.definition.operation else {
        panic!("{:?}", feature.definition.operation);
    };
    let occurrence = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id == place.occurrence)
        .unwrap_or_else(|| panic!("{:?}", ir.model.occurrences));
    assert_eq!(
        occurrence.name.as_deref(),
        Some("fcstd:native:object#Instance")
    );
    (**place).clone()
}

#[test]
fn an_app_link_is_a_place_feature_of_the_group() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="App::Part" name="Product" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Instance" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Product"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Prototype"/></LinkList></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="1" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Instance"><Properties Count="4">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="6" Pz="7" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="FOLLOWS"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let parent = decode(
        &archive(&document.replace("FOLLOWS", "false")),
        DecodeDepth::Design,
    );
    let place = instance_place(&parent);
    assert_eq!(place.base, Some(PlacementBase::Parent));
    let placement = place.placement.as_ref().expect("the link's Placement");
    assert!(placement.inputs.is_empty());
    let origin = placement
        .resolved
        .as_ref()
        .expect("a resolved frame")
        .origin
        .get();
    assert_eq!([origin.x, origin.y, origin.z], [5.0, 6.0, 7.0]);
    let Some(ScaleFactors::PerAxis {
        factors,
        frame: None,
    }) = &place.scale
    else {
        panic!("{:?}", place.scale);
    };
    assert_eq!(
        factors.clone().map(|factor| match factor {
            cadmpeg_ir::design::dimension::Dimension::Literal(value) => value.get(),
            other => panic!("{other:?}"),
        }),
        [2.0, 3.0, 4.0]
    );
    assert!(place.constraints.is_empty());
    // A Place has no result of its own (§5.3).
    assert!(parent.design.scopes[0]
        .current
        .iter()
        .all(|state| state.at != Position::After(feature("fcstd:design:feature#Instance"))));
    let (written, _) = cadir_round_trip(&parent);
    assert_eq!(written.design.features, parent.design.features);

    // The object `Prototype` has an occurrence of its own in `Product`.
    let follows = decode(
        &archive(&document.replace("FOLLOWS", "true")),
        DecodeDepth::Design,
    );
    let place = instance_place(&follows);
    assert_eq!(place.base, Some(PlacementBase::OwnOccurrence));
    assert!(place.placement.is_some());

    // At the identity with no scale, a placement under the parent is
    // absent (§2.4); one after the object's own occurrence is stated
    // (§6.7).
    let at_identity = document
        .replace(r#"Px="5" Py="6" Pz="7""#, r#"Px="0" Py="0" Pz="0""#)
        .replace(
            r#"<PropertyVector valueX="2" valueY="3" valueZ="4"/>"#,
            r#"<PropertyVector valueX="1" valueY="1" valueZ="1"/>"#,
        );
    let place = instance_place(&decode(
        &archive(&at_identity.replace("FOLLOWS", "false")),
        DecodeDepth::Design,
    ));
    assert_eq!(
        (place.placement, place.base, place.scale),
        (None, None, None)
    );
    let place = instance_place(&decode(
        &archive(&at_identity.replace("FOLLOWS", "true")),
        DecodeDepth::Design,
    ));
    assert_eq!(place.base, Some(PlacementBase::OwnOccurrence));
    assert!(place.placement.is_some());
    assert_eq!(place.scale, None);
    // A stated scale keeps the identity placement under the parent: `scale`
    // is stated only with a `placement` (§6.7).
    let place = instance_place(&decode(
        &archive(
            &document
                .replace(r#"Px="5" Py="6" Pz="7""#, r#"Px="0" Py="0" Pz="0""#)
                .replace("FOLLOWS", "false"),
        ),
        DecodeDepth::Design,
    ));
    assert_eq!(place.base, Some(PlacementBase::Parent));
    let origin = place
        .placement
        .as_ref()
        .expect("the link's identity Placement")
        .resolved
        .as_ref()
        .expect("a resolved frame")
        .origin
        .get();
    assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0]);
    let Some(ScaleFactors::PerAxis {
        factors,
        frame: None,
    }) = &place.scale
    else {
        panic!("{:?}", place.scale);
    };
    assert_eq!(
        factors.clone().map(|factor| match factor {
            cadmpeg_ir::design::dimension::Dimension::Literal(value) => value.get(),
            other => panic!("{other:?}"),
        }),
        [2.0, 3.0, 4.0]
    );
}

#[test]
fn a_link_that_follows_an_object_with_no_own_occurrence_is_a_loss() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Link" name="Instance" id="1"/></Objects>
<ObjectData Count="1"><Object name="Instance"><Properties Count="3">
 <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="other.FCStd" name="Prototype"/></Property>
 <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="6" Pz="7" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let place = instance_place(ir);
    assert_eq!(
        (place.placement, place.base, place.scale),
        (None, None, None)
    );
    let unresolved = crate::loss::FreecadLossCode::FeatureLinkPlacementUnresolved.note("");
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == unresolved.code && loss.message.contains("Instance")),
        "{:?}",
        decoded.report().losses
    );
}

#[test]
fn an_app_link_array_places_its_elements_in_one_feature() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Instance"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Instance"><Properties Count="5">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="false"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="placements"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let mut placements = 2_u32.to_le_bytes().to_vec();
    for x in [1.0_f64, 2.0] {
        placements.extend(
            [x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                .into_iter()
                .flat_map(f64::to_le_bytes),
        );
    }
    let ir = decode(
        &archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("placements", &placements),
        ]),
        DecodeDepth::Design,
    );
    let instance = feature("fcstd:design:feature#Instance");
    assert!(group_members(&ir).contains(&instance));
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| Member::Feature(feature.id.clone()) == instance)
        .unwrap_or_else(|| panic!("{:?}", ir.design.features));
    let FeatureOperation::PlaceSet(set) = &feature.definition.operation else {
        panic!("{:?}", feature.definition.operation);
    };
    let elements = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.native_ref.as_deref() == Some("fcstd:native:object#Instance")
        })
        .map(|occurrence| occurrence.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(elements.len(), 2);
    assert_eq!(set.occurrences, elements);
    let (written, _) = cadir_round_trip(&ir);
    assert_eq!(written.design.features, ir.design.features);

    // An array of one element is a `Place` that states only its occurrence
    // (§6.7).
    let mut placement = 1_u32.to_le_bytes().to_vec();
    placement.extend(
        [1.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    let one = decode(
        &archive_entries(&[
            (
                "Document.xml",
                document
                    .replace(r#"<Integer value="2"/>"#, r#"<Integer value="1"/>"#)
                    .as_bytes(),
            ),
            ("placements", &placement),
        ]),
        DecodeDepth::Design,
    );
    let place = instance_place(&one);
    assert_eq!(
        (place.placement, place.base, place.scale),
        (None, None, None)
    );
}

const ASSEMBLY: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="9">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="BracketShape" id="2"/>
 <Object type="Part::Feature" name="PinShape" id="3"/>
 <Object type="App::Link" name="Bracket" id="4"/>
 <Object type="App::Link" name="Pin" id="5"/>
 <Object type="Assembly::JointGroup" name="Joints" id="6"/>
 <Object type="App::FeaturePython" name="Hinge" id="7"/>
 <Object type="App::FeaturePython" name="Ground" id="8"/>
 <Object type="App::FeaturePython" name="Weld" id="9"/>
</Objects>
<ObjectData Count="9">
 <Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="4"><Link value="Bracket"/><Link value="Pin"/><Link value="Joints"/><Link value="Weld"/></LinkList></Property></Properties></Object>
 <Object name="BracketShape"><Properties Count="0"/></Object>
 <Object name="PinShape"><Properties Count="0"/></Object>
 <Object name="Bracket"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="BracketShape"/></Property></Properties></Object>
 <Object name="Pin"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="PinShape"/></Property></Properties></Object>
 <Object name="Joints"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Hinge"/><Link value="Ground"/></LinkList></Property></Properties></Object>
 <Object name="Hinge"><Properties Count="14">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="3"><Enum value="Fixed"/><Enum value="Revolute"/><Enum value="Weld"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="Bracket.Face1"/><Sub value="Bracket.Vertex2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Edge3"/></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0.5" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="-30"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LengthMin" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Ground"><Properties Count="2">
  <Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="Bracket"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
 <Object name="Weld"><Properties Count="3">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="2" CustomEnum="true"/><CustomEnumList count="3"><Enum value="Fixed"/><Enum value="Revolute"/><Enum value="Weld"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;

fn joint_feature<'ir>(
    ir: &'ir CadIr,
    name: &str,
) -> (
    &'ir cadmpeg_ir::design::feature::Feature,
    &'ir cadmpeg_ir::design::ops::shared::Joint,
) {
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("{:?}", ir.design.features));
    let FeatureOperation::Joint(joint) = &feature.definition.operation else {
        panic!("{:?}", feature.definition.operation);
    };
    (feature, joint)
}

/// The construction of frame connector `connector`.
fn frame(
    connector: &cadmpeg_ir::design::ops::shared::JointConnector,
) -> &cadmpeg_ir::design::construction::Construction<cadmpeg_ir::design::construction::kinds::Frame>
{
    let cadmpeg_ir::design::ops::shared::JointConnector::Frame(frame) = connector else {
        panic!("{connector:?}");
    };
    frame
}

/// The native names and occurrence paths of the picks of frame connector
/// `connector`.
fn frame_picks(
    connector: &cadmpeg_ir::design::ops::shared::JointConnector,
) -> Vec<(Option<String>, Vec<String>)> {
    use cadmpeg_ir::design::construction::InputReference;
    use cadmpeg_ir::design::selection::{PickContext, Picks, StepRef};
    let frame = frame(connector);
    let [input] = frame.inputs.as_slice() else {
        panic!("{:?}", frame.inputs);
    };
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("{input:?}");
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("{selection:?}");
    };
    let picks = members
        .as_slice()
        .iter()
        .map(|pick| {
            let native = pick.native.as_ref().map(|name| match &name.value {
                cadmpeg_ir::design::names::NativeNameValue::Text(text) => text.as_str().to_owned(),
                other => panic!("{other:?}"),
            });
            let path = match &pick.context {
                Some(PickContext::Occurrence(path)) => path
                    .0
                    .iter()
                    .map(|step| match &step.occurrence {
                        StepRef::Occurrence(id) => id.as_str().to_owned(),
                        StepRef::Native(name) => panic!("{name:?}"),
                    })
                    .collect(),
                None => Vec::new(),
                Some(other) => panic!("{other:?}"),
            };
            (native, path)
        })
        .collect();
    picks
}

#[test]
fn an_assembly_joint_is_a_joint_feature_of_its_joint_group() {
    use cadmpeg_ir::design::construction::{FrameAxis, PostStep};
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::shared::{AxisLimits, JointKind, JointMotion};
    use cadmpeg_ir::design::options::NativeOption;
    use cadmpeg_ir::design::selection::PickTarget;

    let ir = decode(&archive(ASSEMBLY), DecodeDepth::Design);
    let occurrence = |object: &str| {
        let native = format!("fcstd:native:object#{object}");
        ir.model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence.native_ref.as_deref() == Some(native.as_str())
                    && !occurrence.id.as_str().ends_with(":container")
            })
            .unwrap_or_else(|| panic!("{:?}", ir.model.occurrences))
            .id
            .clone()
    };
    let (bracket, pin) = (occurrence("Bracket"), occurrence("Pin"));
    let assembly = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.native_ref.as_deref() == Some("fcstd:native:object#Assembly"))
        .unwrap()
        .id
        .clone();

    let (feature, hinge) = joint_feature(&ir, "Hinge");
    assert_eq!(feature.id.as_str(), "fcstd:model:joint#Hinge:constraint");
    assert_eq!(feature.suppressed, Some(Dimension::Literal(true)));
    assert_eq!(feature.component.as_ref(), Some(&assembly));
    assert_eq!(
        feature.native_ref.as_deref(),
        Some("fcstd:native:object#Hinge")
    );
    // FreeCAD turns and slides joints about the connector frame's z axis;
    // an enabled limit is in degrees (§6.7).
    let JointKind::Motion {
        motion: JointMotion::Revolute { axis: FrameAxis::Z },
        limits,
        ..
    } = &hinge.kind
    else {
        panic!("{:?}", hinge.kind);
    };
    let [AxisLimits::Rotation {
        axis: FrameAxis::Z,
        limits: Some(bounds),
        rest: None,
    }] = limits.as_slice()
    else {
        panic!("{limits:?}");
    };
    let degrees = |bound: &Option<Dimension<cadmpeg_ir::scalar::Angle>>| match bound {
        Some(Dimension::Literal(angle)) => angle.get().to_degrees(),
        other => panic!("{other:?}"),
    };
    assert!((degrees(&bounds.min) + 30.0).abs() < 1e-9);
    assert!((degrees(&bounds.max) - 45.0).abs() < 1e-9);

    // A connector picks its elements in the occurrence the sub-name names;
    // its offset is its step and its placement its resolved frame.
    let (first, picks) = (
        frame(&hinge.connectors[0]),
        frame_picks(&hinge.connectors[0]),
    );
    let bracket_path = vec![bracket.as_str().to_owned()];
    assert_eq!(
        picks,
        [
            (Some("Face1".to_owned()), bracket_path.clone()),
            (Some("Vertex2".to_owned()), bracket_path.clone())
        ]
    );
    let [PostStep::Fixed(offset)] = first.after.as_slice() else {
        panic!("{:?}", first.after);
    };
    assert_eq!(offset.affine_rows()[2][3], 0.5);
    let origin = first.resolved.as_ref().unwrap().origin.get();
    assert_eq!([origin.x, origin.y, origin.z], [1.0, 0.0, 0.0]);
    let (second, picks) = (
        frame(&hinge.connectors[1]),
        frame_picks(&hinge.connectors[1]),
    );
    assert_eq!(
        picks,
        [(Some("Edge3".to_owned()), vec![pin.as_str().to_owned()])]
    );
    assert!(second.after.is_empty());
    assert_eq!(second.resolved.as_ref().unwrap().origin.get().x, 2.0);

    // A value the joint does not hold is a native option; a detached
    // connector is a source setting.
    let option = |options: &cadmpeg_ir::design::options::NativeOptions, name: &str| {
        options
            .iter()
            .find(|(key, _)| key.as_str() == format!("fcstd:{name}"))
            .map(|(_, option)| option.clone())
    };
    let Some(NativeOption::Angle(Dimension::Literal(angle))) =
        option(&hinge.native_options, "Angle")
    else {
        panic!("{:?}", hinge.native_options);
    };
    assert!((angle.get().to_degrees() - 15.0).abs() < 1e-9);
    let Some(NativeOption::Length(Dimension::Literal(length))) =
        option(&hinge.native_options, "LengthMin")
    else {
        panic!("{:?}", hinge.native_options);
    };
    assert_eq!(length.get(), 1.0);
    for used in [
        "AngleMin",
        "AngleMax",
        "EnableAngleMin",
        "Suppressed",
        "Detach1",
    ] {
        assert_eq!(option(&hinge.native_options, used), None, "{used}");
    }
    assert_eq!(
        option(&hinge.source_settings, "Detach1"),
        Some(NativeOption::Bool(Dimension::Literal(true)))
    );

    // An unknown family is a native kind; a reference with no element picks
    // the whole occurrence.
    let (weld_feature, weld) = joint_feature(&ir, "Weld");
    assert_eq!(
        weld.kind,
        JointKind::Native(cadmpeg_core::text::NonBlankString::new("Weld".to_owned()).unwrap())
    );
    let picks = frame_picks(&weld.connectors[0]);
    assert_eq!(picks, [(None, bracket_path)]);
    let cadmpeg_ir::design::construction::InputReference::Picks(selection) =
        frame(&weld.connectors[0]).inputs[0]
            .reference
            .as_ref()
            .unwrap()
    else {
        unreachable!();
    };
    let cadmpeg_ir::design::selection::Picks::Named(members) = &selection.picks else {
        unreachable!();
    };
    assert!(members
        .as_slice()
        .iter()
        .all(|pick| pick.target == PickTarget::Component));

    // A JointGroup is an unordered Group of its joints; a joint it does not
    // list is a member of the document's Group.
    let joints = ir
        .design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Joints")
        .unwrap_or_else(|| panic!("{:?}", ir.design.containers));
    let Container::Unordered {
        kind: UnorderedKind::Group,
        members,
        ..
    } = joints
    else {
        panic!("{joints:?}");
    };
    assert_eq!(
        members,
        &BTreeSet::from([Member::Feature(feature.id.clone())])
    );
    assert!(ir.design.scopes[0].roots.contains(joints.id()));
    assert!(group_members(&ir).contains(&Member::Feature(weld_feature.id.clone())));

    // Grounding is occurrence state (§6.7).
    let scope = &ir.design.scopes[0];
    assert_eq!(
        scope
            .occurrences
            .get(&bracket)
            .and_then(|state| state.grounded),
        Some(true)
    );
    assert_eq!(scope.occurrences.len(), 1);

    let (written, omitted) = cadir_round_trip(&ir);
    for id in [feature.id.as_str(), weld_feature.id.as_str()] {
        assert!(!omitted.contains(id), "{omitted:?}");
    }
    assert_eq!(written.design.features, ir.design.features);
    assert_eq!(written.design.containers, ir.design.containers);
    assert_eq!(written.design.scopes, ir.design.scopes);
}
