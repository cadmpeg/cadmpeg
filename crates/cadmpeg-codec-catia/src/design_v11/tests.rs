// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of decoded CATIA parts.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::construction::{InputReference, Relation, Subject};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::ops::sketch::{SketchPlane, SketchRelation};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{Anchor, PickTarget, Picks, PrincipalEntity};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::{CadIr, ReadOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_container::outer_container_catpart;
use crate::test_support::test_feature_tree::{
    compact, part, stream, stream_with_catalog, stream_with_sketches, SKETCH,
};
use crate::test_support::test_formula::standard_catpart_with_formula_relation;
use crate::test_support::test_object_graph::{
    catalog_stream, entity_backed_object_graph, object_graph_record,
};
use crate::test_support::test_sketch_bindings::placed_sketch_catpart;
use crate::CatiaCodec;

fn decode(bytes: Vec<u8>, options: &DecodeOptions) -> EditableDecodeResult {
    EditableDecodeResult::from(CatiaCodec.decode(&mut Cursor::new(bytes), options).unwrap())
}

fn principal_plane_part(class: &str) -> Vec<u8> {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut stream = entity_backed_object_graph(&records, &[2, 3]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        class,
    ]));
    outer_container_catpart(&stream).0
}

fn design_findings(result: &EditableDecodeResult) -> Vec<String> {
    cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .unwrap()
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| format!("{finding:?}"))
        .collect()
}

/// The one root definition of an unscoped part owns every body (§8.1).
fn assert_root_owns_every_body(ir: &cadmpeg_ir::CadIr) {
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(root.id.as_str(), "catia:model:product-definition#root");
    assert_eq!(
        root.bodies,
        ir.geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_part_with_parameters_states_one_scope_over_its_root_definition() {
    let result = decode(
        standard_catpart_with_formula_relation(4, false),
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    assert_eq!(ir.model.parameters.len(), 2);
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(root.id.as_str(), "catia:model:product-definition#root");
    assert!(!ir.geometry.bodies.is_empty());
    assert_root_owns_every_body(ir);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(scope.definition, root.id);
    assert!(scope.roots.is_empty());
    assert!(scope.current.is_empty());
    // The decoder reads no member that produces a body: every body of the
    // root is unproduced, and a loss names the unread history (§5.5).
    assert_eq!(
        scope.unproduced.iter().collect::<Vec<_>>(),
        root.bodies.iter().collect::<Vec<_>>()
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::HistoryFeaturesUnread
            .note("")
            .code));
    assert_eq!(
        ir.design
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, &parameter.scope))
            .collect::<Vec<_>>(),
        ir.model
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, &root.id))
            .collect::<Vec<_>>()
    );
    assert!(ir.design.features.is_empty());
    assert!(ir.design.containers.is_empty());
    assert_eq!(design_findings(&result), Vec::<String>::new());
}

/// The entity identity that keys a tree record's design identity.
fn entity(id: &str) -> u32 {
    id.rsplit('-').next().unwrap().parse().unwrap()
}

/// A member as its record kind and entity identity.
fn member(member: &Member) -> (&'static str, u32) {
    match member {
        Member::Feature(id) => ("feature", entity(id.as_str())),
        Member::Container(id) => ("container", entity(id.as_str())),
    }
}

fn container<'ir>(ir: &'ir CadIr, prefix: &str, entity_id: u32) -> &'ir Container {
    ir.design
        .containers
        .iter()
        .find(|container| {
            container.id().as_str().starts_with(prefix)
                && entity(container.id().as_str()) == entity_id
        })
        .unwrap()
}

/// A feature's operation by its native kind or its unresolved family.
fn operation(operation: &FeatureOperation) -> String {
    match operation {
        FeatureOperation::Native { kind, .. } => kind.as_str().to_owned(),
        FeatureOperation::Unresolved { family } => format!("unresolved {family:?}"),
        other => panic!("an untyped operation: {other:?}"),
    }
}

/// The principal entity and the anchor of an origin datum's one
/// `Coincident` input (§6.2, Origin features).
fn origin_plane(operation: &FeatureOperation) -> (PrincipalEntity, Anchor) {
    let FeatureOperation::Datum(datum) = operation else {
        panic!("an origin feature is a datum: {operation:?}");
    };
    let DatumElement::Plane(plane) = &datum.element else {
        panic!("a plane datum");
    };
    assert!(plane.after.is_empty());
    let [input] = plane.inputs.as_slice() else {
        panic!("one input");
    };
    let role = input.role.as_ref().unwrap();
    assert_eq!(
        (&role.subject, &role.relation),
        (&Subject::Whole, &Relation::Coincident)
    );
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("the input picks");
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("a named pick");
    };
    let [picked] = members.as_slice() else {
        panic!("one pick");
    };
    let PickTarget::Principal {
        entity,
        container: None,
    } = &picked.target
    else {
        panic!("a principal pick");
    };
    (*entity, selection.at.clone())
}

/// Write the document as CADIR and read it back: the read document and the
/// write's losses that name a design record.
fn cadir_round_trip(ir: &CadIr) -> (CadIr, Vec<String>) {
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
        .unwrap();
    let design_ids = ir
        .design
        .features
        .iter()
        .map(|feature| feature.id.as_str().to_owned())
        .chain(
            ir.design
                .containers
                .iter()
                .map(|container| container.id().as_str().to_owned()),
        )
        .chain(
            ir.design
                .sketches
                .iter()
                .map(|sketch| sketch.id.as_str().to_owned()),
        )
        .chain(
            ir.design
                .sketch_constraints
                .iter()
                .map(|constraint| constraint.id.as_str().to_owned()),
        )
        .collect::<Vec<_>>();
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "arena.omitted")
        .filter(|loss| {
            design_ids
                .iter()
                .any(|id| loss.message.contains(id.as_str()))
        })
        .map(|loss| loss.message.clone())
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let (read, _, _) = CadIr::read(&bytes, &ReadOptions::default())
        .unwrap()
        .without_sidecar();
    (read, omitted)
}

fn assert_design_survives_cadir(ir: &CadIr) {
    let (read, omitted) = cadir_round_trip(ir);
    assert_eq!(omitted, Vec::<String>::new());
    assert_eq!(read.design.scopes, ir.design.scopes);
    assert_eq!(read.design.containers, ir.design.containers);
    assert_eq!(read.design.features, ir.design.features);
    assert_eq!(read.design.sketches, ir.design.sketches);
    assert_eq!(read.design.sketch_constraints, ir.design.sketch_constraints);
}

/// The part root lists a plane, the body `MainTool` selects, and an empty
/// `MechanicalTool`; the main body lists two pads (§5.2).
#[test]
fn a_part_root_states_its_group_and_its_bodies_in_list_order() {
    let result = decode(
        outer_container_catpart(&stream(&part())).0,
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(
        scope
            .roots
            .iter()
            .map(|id| (id.as_str().split('#').next().unwrap(), entity(id.as_str())))
            .collect::<Vec<_>>(),
        [
            ("catia:outer:feature-group", 2),
            ("catia:outer:feature-container", 3),
            ("catia:outer:feature-container", 7),
        ]
    );
    let Container::Unordered { kind, members, .. } = container(ir, "catia:outer:feature-group", 2)
    else {
        panic!("the part's group is unordered");
    };
    assert_eq!(*kind, UnorderedKind::Group);
    assert_eq!(
        members.iter().map(member).collect::<Vec<_>>(),
        [("feature", 4)]
    );
    for (entity_id, listed) in [(3, vec![("feature", 5), ("feature", 6)]), (7, vec![])] {
        let Container::Ordered { kind, members, .. } =
            container(ir, "catia:outer:feature-container", entity_id)
        else {
            panic!("a body is ordered");
        };
        assert_eq!(*kind, OrderedKind::Body);
        assert_eq!(members.iter().map(member).collect::<Vec<_>>(), listed);
    }
    assert_eq!(
        ir.design
            .features
            .iter()
            .map(|feature| {
                (
                    feature.id.as_str().split('#').next().unwrap(),
                    entity(feature.id.as_str()),
                    operation(&feature.definition.operation),
                    feature.native_ref.is_some(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                "catia:outer:feature",
                4,
                "unresolved Datum".to_owned(),
                true
            ),
            ("catia:outer:feature", 5, "catia:Pad".to_owned(), true),
            ("catia:outer:feature", 6, "catia:Pad".to_owned(), true),
        ]
    );
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// The rollback of each body of a part whose `CurrentFeature` selects the
/// entity `current`, by the body's entity.
fn rollbacks(current: u8) -> Vec<(u32, Option<Rollback>)> {
    let mut records = part();
    assert_eq!(records[0].2[19], 0x86, "the in-work token");
    records[0].2[19] = current;
    let result = decode(
        outer_container_catpart(&stream(&records)).0,
        &DecodeOptions::default(),
    );
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(result.ir());
    result
        .ir()
        .design
        .containers
        .iter()
        .filter_map(|container| match container {
            Container::Ordered { id, rollback, .. } => {
                Some((entity(id.as_str()), rollback.clone()))
            }
            Container::Unordered { .. } => None,
        })
        .collect()
}

/// The in-work object is the rollback of the body that lists it: before the
/// member after it, or the body's end where it is the last member or the
/// body itself (§5.2). Every other body's rollback is unread.
#[test]
fn the_in_work_object_is_the_rollback_of_the_body_that_lists_it() {
    let feature = |entity_id: u32| {
        let Some(Container::Ordered { members, .. }) = decode(
            outer_container_catpart(&stream(&part())).0,
            &DecodeOptions::default(),
        )
        .ir()
        .design
        .containers
        .iter()
        .find(|container| entity(container.id().as_str()) == 3)
        .cloned() else {
            panic!("the main body is ordered");
        };
        members
            .into_iter()
            .find(|member| self::member(member) == ("feature", entity_id))
            .unwrap()
    };
    assert_eq!(
        rollbacks(0x86),
        [(3, Some(Rollback::Before(feature(6)))), (7, None)]
    );
    assert_eq!(rollbacks(0x87), [(3, Some(Rollback::End)), (7, None)]);
    assert_eq!(rollbacks(0x88), [(3, None), (7, Some(Rollback::End))]);
    assert_eq!(rollbacks(0x85), [(3, None), (7, None)]);
}

/// A `HybridBody` that `MainTool` does not select is a set whose kind is
/// unread: neither it nor its members are stated, and a loss names it.
#[test]
fn a_set_whose_kind_is_unread_is_unstated() {
    let mut records = part();
    assert_eq!(records[0].2[21], 0x84, "the main-body token");
    records[0].2[21] = 0x88;
    let result = decode(
        outer_container_catpart(&stream(&records)).0,
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(
        scope
            .roots
            .iter()
            .map(|id| (id.as_str().split('#').next().unwrap(), entity(id.as_str())))
            .collect::<Vec<_>>(),
        [
            ("catia:outer:feature-group", 2),
            ("catia:outer:feature-container", 7),
        ]
    );
    assert_eq!(
        ir.design
            .features
            .iter()
            .map(|feature| entity(feature.id.as_str()))
            .collect::<Vec<_>>(),
        [4]
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::DesignTreeRecordsUnstated
            .note("")
            .code
        && loss
            .message
            .contains("is a set whose ordered or unordered kind is unread")));
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// A member whose compact class selects a blank catalog entry is not
/// stated, and the loss names it.
#[test]
fn a_member_whose_class_is_blank_is_unstated() {
    let mut records = part();
    records[3].1 = compact(0x08, 0x37);
    let result = decode(
        outer_container_catpart(&stream(&records)).0,
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    let Container::Ordered { members, .. } = container(ir, "catia:outer:feature-container", 3)
    else {
        panic!("the main body is ordered");
    };
    assert_eq!(
        members.iter().map(member).collect::<Vec<_>>(),
        [("feature", 6)]
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::DesignTreeRecordsUnstated
            .note("")
            .code
        && loss.message.contains("selects a blank catalog entry")));
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// A feature the decoder reads from a record whose compact class
/// contradicts the reading is not restated, and a loss names it.
#[test]
fn a_feature_whose_record_class_contradicts_the_reading_is_not_restated() {
    let catalog = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "xy-plane",
        "Pad",
    ];
    let records = [
        (2, compact(0x08, 0x39), vec![0xfe]),
        (3, compact(0x08, 0x39), vec![0xfe]),
    ];
    let result = decode(
        outer_container_catpart(&stream_with_catalog(&records, &catalog)).0,
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    let [legacy] = ir.model.features.as_slice() else {
        panic!("one typed feature");
    };
    assert!(ir.design.features.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::DesignFeaturesContradicted
            .note("")
            .code
        && loss.message.contains(legacy.id.as_str())
        && loss.message.contains("record class Pad")));
}

/// A part that stores no part root keeps each feature the decoder types as a
/// member of the part's group, and a loss names it.
#[test]
fn a_feature_no_read_set_lists_is_a_member_of_the_part_group() {
    let result = decode(principal_plane_part("xy-plane"), &DecodeOptions::default());
    let ir = result.ir();
    let [legacy] = ir.model.features.as_slice() else {
        panic!("one typed feature");
    };
    assert_root_owns_every_body(ir);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(
        scope
            .roots
            .iter()
            .map(cadmpeg_ir::design::ids::ContainerId::as_str)
            .collect::<Vec<_>>(),
        ["catia:outer:feature-group#root"]
    );
    let [Container::Unordered { kind, members, .. }] = ir.design.containers.as_slice() else {
        panic!("one unordered group");
    };
    assert_eq!(*kind, UnorderedKind::Group);
    assert_eq!(
        members.iter().collect::<Vec<_>>(),
        [&Member::Feature(legacy.id.clone())]
    );
    let [feature] = ir.design.features.as_slice() else {
        panic!("one design feature");
    };
    assert_eq!(feature.id, legacy.id);
    assert_eq!(
        origin_plane(&feature.definition.operation),
        (PrincipalEntity::XyPlane, Anchor::Current)
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::DesignFeaturesUngrouped
            .note("")
            .code
        && loss.message.contains(legacy.id.as_str())));
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// The part's xy, yz, and zx planes are origin features on the matching
/// principal planes, members of the part's group (§6.2).
#[test]
fn the_part_planes_are_origin_datums_on_the_principal_planes() {
    for (class, entity) in [
        ("xy-plane", PrincipalEntity::XyPlane),
        ("yz-plane", PrincipalEntity::YzPlane),
        ("zx-plane", PrincipalEntity::XzPlane),
    ] {
        let result = decode(principal_plane_part(class), &DecodeOptions::default());
        let ir = result.ir();
        let [feature] = ir.design.features.as_slice() else {
            panic!("one design feature");
        };
        assert_eq!(
            origin_plane(&feature.definition.operation),
            (entity, Anchor::Current)
        );
        assert_eq!(design_findings(&result), Vec::<String>::new());
        assert_design_survives_cadir(ir);
    }
}

/// The plane construction of a design sketch: its inputs and its resolved
/// frame.
fn sketch_plane(
    sketch: &cadmpeg_ir::design::ops::sketch::Sketch,
) -> &cadmpeg_ir::design::construction::Construction<cadmpeg_ir::design::construction::kinds::Frame>
{
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("a sketch on a plane");
    };
    assert!(plane.after.is_empty());
    plane
}

/// A sketch record of the part's tree is a sketch feature whose sketch has
/// one plane input with unread picks, anchored by the set that lists it
/// (§5.8, §6.1).
#[test]
fn a_tree_sketch_is_a_sketch_feature_whose_plane_is_unread() {
    let mut records = part();
    records[2].1 = compact(0x0c, SKETCH);
    records[3].1 = compact(0x08, SKETCH);
    let result = decode(
        outer_container_catpart(&stream_with_sketches(&records)).0,
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    let sketches = ir
        .design
        .features
        .iter()
        .filter_map(|feature| match &feature.definition.operation {
            FeatureOperation::Sketch(id) => Some((entity(feature.id.as_str()), id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sketches
            .iter()
            .map(|(feature, id)| (
                *feature,
                id.as_str().split('#').next().unwrap(),
                entity(id.as_str())
            ))
            .collect::<Vec<_>>(),
        [(4, "catia:outer:sketch", 4), (5, "catia:outer:sketch", 5)]
    );
    assert_eq!(
        ir.design
            .sketches
            .iter()
            .map(|sketch| {
                let plane = sketch_plane(sketch);
                assert_eq!(plane.resolved, None);
                let [input] = plane.inputs.as_slice() else {
                    panic!("one plane input");
                };
                assert_eq!(input.role, None);
                let Some(InputReference::Picks(selection)) = &input.reference else {
                    panic!("the plane input picks");
                };
                assert_eq!(selection.picks, Picks::Unread);
                assert_eq!(
                    sketch.solve.status,
                    cadmpeg_ir::design::evaluation::EvaluationStatus::Current
                );
                assert!(sketch.native_ref.is_some());
                (
                    entity(sketch.id.as_str()),
                    entity(sketch.owner.as_str()),
                    selection.at.clone(),
                )
            })
            .collect::<Vec<_>>(),
        [(4, 4, Anchor::Current), (5, 5, Anchor::Input)]
    );
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// A sketch whose placement the decoder resolves keeps its frame: the
/// origin, the u axis as x, the normal as z, and z × x as y (§6.1).
#[test]
fn a_decoded_sketch_keeps_its_resolved_frame() {
    let result = decode(placed_sketch_catpart(), &DecodeOptions::default());
    let ir = result.ir();
    let [legacy] = ir.model.sketches.as_slice() else {
        panic!("one decoded sketch");
    };
    let (origin, normal, u_axis) = legacy.resolved_placement().unwrap();
    let [feature] = ir.design.features.as_slice() else {
        panic!("one design feature");
    };
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::Sketch(legacy.id.clone())
    );
    let [sketch] = ir.design.sketches.as_slice() else {
        panic!("one design sketch");
    };
    assert_eq!(sketch.id, legacy.id);
    assert_eq!(sketch.owner, feature.id);
    assert_eq!(sketch.native_ref, legacy.native_ref);
    let plane = sketch_plane(sketch);
    assert!(plane.inputs.is_empty());
    let frame = plane.resolved.as_ref().unwrap();
    let unit = |v: [f64; 3]| {
        let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / length)
    };
    let (u_axis, normal) = (u_axis.as_raw(), normal.as_raw());
    let (x, z) = (
        unit([u_axis.x, u_axis.y, u_axis.z]),
        unit([normal.x, normal.y, normal.z]),
    );
    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];
    assert_eq!(frame.origin, origin);
    for (axis, expected) in [(&frame.x_axis, x), (&frame.y_axis, y), (&frame.z_axis, z)] {
        let axis = axis.as_raw();
        for (got, want) in [axis.x, axis.y, axis.z].into_iter().zip(expected) {
            assert!((got - want).abs() < 1e-12, "{got} against {want}");
        }
    }
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
}

/// A native constraint of a decoded sketch is a `Native` relation written
/// `catia:<kind>`, whose native properties and operands are text options
/// (§6.1, Rule N).
#[test]
fn a_native_sketch_constraint_is_a_native_relation_of_its_sketch() {
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::options::NativeOption;
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchNativeOperand,
    };

    let mut ir = decode(placed_sketch_catpart(), &DecodeOptions::default())
        .ir()
        .clone();
    let sketch = ir.model.sketches[0].id.clone();
    let operand = SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("Length"),
        field: None,
        object_index: Some(3),
        native_ref: Some("catia:outer:entity-record#source".to_owned()),
    };
    let properties =
        std::collections::BTreeMap::from([("catia_framing".to_owned(), "range".to_owned())]);
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint(
            "catia:outer:sketch-constraint#0000000000-0000000009".to_owned(),
        )
        .unwrap(),
        sketch: sketch.clone(),
        definition: SketchConstraintDefinition::native_with_operand(
            cadmpeg_core::nonblank_literal!("Distance"),
            properties.clone(),
            Vec::new(),
            operand.clone(),
        ),
        name: None,
        driving: None,
        active: Some(true),
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some("catia:outer:entity-record#range".to_owned()),
    });
    ir.design = cadmpeg_ir::document::Design::default();
    let mut losses = Vec::new();
    crate::test_support::with_service_context(|ctx| {
        crate::design_v11::project(
            ctx,
            &mut ir,
            &crate::native::CatiaNative::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
            &mut losses,
        )
    })
    .unwrap();
    let [constraint] = ir.design.sketch_constraints.as_slice() else {
        panic!("one design constraint");
    };
    assert_eq!(constraint.id, ir.model.sketch_constraints[0].id);
    assert_eq!(constraint.sketch, sketch);
    assert_eq!(
        constraint.relation,
        SketchRelation::Native {
            kind: cadmpeg_core::nonblank_literal!("catia:Distance"),
            operands: Vec::new(),
            values: Vec::new(),
        }
    );
    assert_eq!(constraint.active, Some(Dimension::Literal(true)));
    assert_eq!(
        constraint.native_ref.as_deref(),
        Some("catia:outer:entity-record#range")
    );
    let text = |key: &str| {
        let Some(NativeOption::Text(Dimension::Literal(text))) = constraint.native_options.get(
            &cadmpeg_ir::design::names::CodecName::new(&cadmpeg_ir::codec_id!("catia"), key)
                .unwrap(),
        ) else {
            panic!("a text option {key}");
        };
        text.as_str().to_owned()
    };
    assert_eq!(
        serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text("properties"))
            .unwrap(),
        properties
    );
    assert_eq!(
        serde_json::from_str::<Vec<SketchNativeOperand>>(&text("operands")).unwrap(),
        [operand]
    );
    assert_design_survives_cadir(&ir);
}

#[test]
fn a_part_with_no_history_states_no_design_record() {
    let result = decode(
        crate::test_support::test_container::standard_catpart(),
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    assert!(ir.model.features.is_empty());
    assert!(ir.model.parameters.is_empty());
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.parameters.is_empty());
    assert_root_owns_every_body(ir);
}

#[test]
fn a_geometry_decode_states_no_design_record() {
    let options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Geometry,
        ..DecodeOptions::default()
    };
    let result = decode(standard_catpart_with_formula_relation(4, false), &options);
    let ir = result.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.parameters.is_empty());
    assert_root_owns_every_body(ir);
}
