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

/// A configuration whose body list its evaluation holds writes its own
/// partition from that evaluation's records. A write with no retained or
/// patched partition reaches it; the write tests below take that path.
#[test]
fn a_stored_configuration_state_writes_its_partition() {
    let result = decode(native_envelope_fixture(&two_configurations()));
    let ir = result.ir();
    let first = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("First"))
        .unwrap();
    assert_eq!(first.bodies, None);
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation");
    };
    let stored = &evaluation.content.as_ref().unwrap().delta.records;
    let sections = crate::writer::configuration_partitions(ir, 0.001, &mut Vec::new()).unwrap();
    let section = format!("Contents/Config-{}-Partition", first.source_index.unwrap());
    let (_, payload) = sections
        .iter()
        .find(|(name, _)| *name == section)
        .unwrap_or_else(|| {
            panic!(
                "{section} in {:?}",
                sections.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        });
    let mut written = outer_header();
    written.extend(make_block(0x20, &section, payload));
    let reread = decode(written);
    assert_eq!(reread.ir().geometry.bodies.len(), stored.bodies.len());
    assert_eq!(reread.ir().geometry.faces.len(), stored.faces.len());
}

/// The losses of a write of `ir` with no retained source, and the bytes.
fn write_without_source(
    ir: &cadmpeg_ir::CadIr,
) -> (Vec<cadmpeg_ir::report::loss::LossNote>, Vec<u8>) {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut written = Vec::new();
    let report = crate::SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut written)
        .unwrap();
    (report.losses, written)
}

/// The body and face counts of each state the design stores.
fn stored_states(ir: &cadmpeg_ir::CadIr) -> Vec<(usize, usize)> {
    ir.design
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref())
        .map(|content| {
            let records = &content.delta.records;
            (records.bodies.len(), records.faces.len())
        })
        .collect()
}

/// The message of the write's note on the partitions it writes with no body.
fn bodiless_note(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Option<&str> {
    losses
        .iter()
        .find(|loss| {
            loss.code.local_code() == "record.residual-unavailable"
                && loss.message.contains("which hold no body")
        })
        .map(|loss| loss.message.as_str())
}

/// A part with two configurations written with no retained source reads
/// back with the active configuration's bodies in the geometry and the
/// other configuration's bodies in its evaluation.
#[test]
fn a_part_written_without_its_source_keeps_every_configuration_state() {
    let result = decode(two_configurations());
    let ir = result.ir();
    assert!(matches!(stored_states(ir).as_slice(), [(1, faces)] if *faces > 0));
    let (_, written) = write_without_source(ir);
    let reread = decode(written);
    let back = reread.ir();
    assert_eq!(back.geometry.bodies.len(), ir.geometry.bodies.len());
    assert_eq!(back.geometry.faces.len(), ir.geometry.faces.len());
    assert_eq!(stored_states(back), stored_states(ir));
}

/// The two configurations of [`two_configurations`], with the partition of
/// the active one, `Second`, an empty partition/deltas model; `stored`
/// gives `First` a one-triangle body, else an empty model too.
fn faceless_active_configuration(stored: bool) -> Vec<u8> {
    let mut partitions = outer_header();
    let first = if stored {
        parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        )
    } else {
        let mut empty = parasolid_with_body("partition body", "SCH_SW_33103_11000", &[]);
        empty.extend(parasolid_with_body(
            "deltas body",
            "SCH_SW_33103_11000",
            &[],
        ));
        empty
    };
    partitions.extend(make_block(0x20, "Contents/Config-0-Partition", &first));
    let mut empty = parasolid_with_body("partition body", "SCH_SW_33103_11000", &[]);
    empty.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &[],
    ));
    partitions.extend(make_block(0x21, "Contents/Config-1-Partition", &empty));
    two_configurations_of(partitions)
}

/// Where the active configuration holds no face, a write with no retained
/// source still writes the state another configuration stores, and its note
/// names only the partition that holds no body.
#[test]
fn a_faceless_active_configuration_keeps_the_stored_state_of_another() {
    let result = decode(faceless_active_configuration(true));
    let ir = result.ir();
    assert!(ir.geometry.faces.is_empty());
    assert!(matches!(stored_states(ir).as_slice(), [(1, faces)] if *faces > 0));
    let (losses, written) = write_without_source(ir);
    let note = bodiless_note(&losses).unwrap_or_else(|| panic!("{losses:#?}"));
    assert!(note.contains("Contents/Config-1-Partition"), "{note}");
    assert!(!note.contains("Contents/Config-0-Partition"), "{note}");
    assert!(
        !losses
            .iter()
            .any(|loss| loss.code.local_code() == "output.empty"),
        "{losses:#?}"
    );
    let reread = decode(written);
    assert_eq!(stored_states(reread.ir()), stored_states(ir));
}

/// Where no configuration stores a body, every partition is written with
/// no body, and the note names each of them.
#[test]
fn a_part_with_no_face_writes_every_partition_bodiless() {
    let result = decode(faceless_active_configuration(false));
    let ir = result.ir();
    assert!(ir.geometry.faces.is_empty());
    assert!(stored_states(ir).iter().all(|(bodies, _)| *bodies == 0));
    let (losses, _) = write_without_source(ir);
    let note = bodiless_note(&losses).unwrap_or_else(|| panic!("{losses:#?}"));
    assert!(note.contains("Contents/Config-0-Partition"), "{note}");
    assert!(note.contains("Contents/Config-1-Partition"), "{note}");
    assert!(
        losses
            .iter()
            .any(|loss| loss.code.local_code() == "output.empty"),
        "{losses:#?}"
    );
}
