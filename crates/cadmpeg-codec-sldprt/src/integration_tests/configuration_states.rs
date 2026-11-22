// SPDX-License-Identifier: Apache-2.0
//! The current geometry of a part holds the active configuration's bodies;
//! the bodies another configuration stores are its evaluation's.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use super::{assert_valid, decode};
use crate::loss::SldprtLossCode;
use crate::test_support::container::{
    make_block, native_envelope_fixture, outer_header, sldprt_with_colliding_sites,
};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::{owned_triangle, parasolid_with_body};
use crate::test_support::tessellation::display_list_payload;

/// Two configurations, each with its own one-triangle partition; `Second`,
/// whose partition is `Config-1-Partition`, is active.
fn two_configurations() -> Vec<u8> {
    two_configurations_of(sldprt_with_colliding_sites())
}

/// The two configurations of [`two_configurations`] over the partitions
/// that `source` holds.
fn two_configurations_of(mut source: Vec<u8>) -> Vec<u8> {
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket">
    <Configuration Name="First" SourceIndex="0"/>
    <Configuration Name="Second" SourceIndex="1"/>
    <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
</Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[]),
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));
    source
}

fn ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> BTreeSet<&'a str> {
    ids.into_iter().collect()
}

#[test]
fn the_current_geometry_holds_the_active_configuration_alone() {
    let result = decode(native_envelope_fixture(&two_configurations()));
    let ir = result.ir();
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let [current] = scope.current.as_slice() else {
        panic!("one current state");
    };
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", ir.design.evaluations);
    };
    let stored = &evaluation.content.as_ref().unwrap().delta.records;
    assert_eq!(stored.bodies.len(), 1);
    assert_eq!(
        ids(ir.geometry.bodies.iter().map(|body| body.id.as_str())),
        ids(current.bodies.iter().map(cadmpeg_ir::ids::BodyId::as_str))
    );
    let geometry = ids(ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .chain(ir.geometry.points.iter().map(|point| point.id.as_str())));
    assert!(stored
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .chain(stored.points.iter().map(|point| point.id.as_str()))
        .all(|id| !geometry.contains(id)));
    let first = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("First"))
        .unwrap();
    assert_eq!(first.bodies, None);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SldprtLossCode::ConfigIncoherentBodyRefs.kind()));
    assert_valid(&result);
}

#[test]
fn a_written_part_keeps_the_bodies_of_every_configuration() {
    let result = decode(native_envelope_fixture(&two_configurations()));
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(result.ir(), result.source_fidelity(), &mut written)
        .unwrap();
    let rewritten = decode(written);
    let ir = rewritten.ir();
    let stored = ir
        .design
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref())
        .map(|content| content.delta.records.bodies.len())
        .collect::<Vec<_>>();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(stored, [1]);
}

/// The display table of a face that the active configuration's partition and
/// the other configuration's partition both hold, at the same place, binds to
/// the active body: the display tables are the active configuration's.
#[test]
fn a_display_table_binds_to_the_active_copy_of_a_stored_face() {
    let mut partitions = outer_header();
    for (index, owner) in [(0, 700), (1, 701)] {
        partitions.extend(make_block(
            0x20 + index,
            &format!("Contents/Config-{index}-Partition"),
            &parasolid_with_body(
                "partition body",
                "SCH_SW_33103_11000",
                &owned_triangle(0, owner, 0.0),
            ),
        ));
    }
    let mut source = two_configurations_of(partitions);
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &display_list_payload(),
    ));
    let result = decode(native_envelope_fixture(&source));
    let ir = result.ir();
    let [body] = ir.geometry.bodies.as_slice() else {
        panic!("one current body: {:#?}", ir.geometry.bodies);
    };
    let [face] = ir.geometry.faces.as_slice() else {
        panic!("one current face");
    };
    let [mesh] = ir.model.tessellations.as_slice() else {
        panic!("one display table: {:#?}", ir.model.tessellations);
    };
    assert_eq!(mesh.body.as_ref(), Some(&body.id));
    assert_eq!(mesh.faces, std::slice::from_ref(&face.id));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SldprtLossCode::ConfigStatePresentationUnstated.kind()));
    assert_valid(&result);
}
