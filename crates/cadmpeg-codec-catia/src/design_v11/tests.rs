// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of decoded CATIA parts.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::{CadIr, ReadOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_container::outer_container_catpart;
use crate::test_support::test_feature_tree::{part, stream};
use crate::test_support::test_formula::standard_catpart_with_formula_relation;
use crate::test_support::test_object_graph::{
    catalog_stream, entity_backed_object_graph, object_graph_record,
};
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
                let FeatureOperation::Native { kind, .. } = &feature.definition.operation else {
                    panic!("an untyped feature is native");
                };
                (
                    feature.id.as_str().split('#').next().unwrap(),
                    entity(feature.id.as_str()),
                    kind.as_str(),
                    feature.native_ref.is_some(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("catia:outer:feature", 4, "catia:GSMPlane", true),
            ("catia:outer:feature", 5, "catia:Pad", true),
            ("catia:outer:feature", 6, "catia:Pad", true),
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
        feature.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Datum
        }
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::CatiaLossCode::DesignFeaturesUngrouped
            .note("")
            .code
        && loss.message.contains(legacy.id.as_str())));
    assert_eq!(design_findings(&result), Vec::<String>::new());
    assert_design_survives_cadir(ir);
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
