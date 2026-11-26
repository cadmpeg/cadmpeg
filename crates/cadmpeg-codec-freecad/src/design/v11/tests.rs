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
    let identity = body_placement(&identity).expect("a stored identity placement");
    let frame = identity.resolved.as_ref().expect("a resolved frame");
    let origin = frame.origin.get();
    assert!(near([origin.x, origin.y, origin.z], [0.0; 3]));
    assert!(near(axis(frame.x_axis), [1.0, 0.0, 0.0]));
    assert!(near(axis(frame.z_axis), [0.0, 0.0, 1.0]));
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
        FeatureOperation::Unresolved {
            family: OperationFamily::Spreadsheet
        }
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
}
