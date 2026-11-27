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
use crate::test_support::parasolid::{
    owned_triangle, owned_triangle_with_resolution, parasolid_with_body,
};
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
    assert_eq!(mesh.of.body(), Some(&body.id));
    assert_eq!(mesh.of.faces(), std::slice::from_ref(&face.id));
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
    let sections = crate::writer::configuration_partitions(
        ir,
        0.001,
        cadmpeg_parasolid::precision::DEFAULT_LINEAR_RESOLUTION,
    )
    .unwrap()
    .sections;
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
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
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
    let (losses, written) = write_without_source(ir);
    assert!(
        !losses
            .iter()
            .any(|loss| loss.code.local_code() == "writer.bodies-omitted"),
        "{losses:#?}"
    );
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

/// A part written without its source reads back at the linear tolerance it
/// was written with, also where a configuration other than the active one
/// stores the bodies.
#[test]
fn every_configuration_written_without_its_source_states_the_written_linear_tolerance() {
    for (source, linear) in [
        (faceless_active_configuration(true), None),
        (two_configurations(), Some(1.0e-7)),
    ] {
        let result = decode(source);
        let mut ir = result.ir().clone();
        if let Some(linear) = linear {
            ir.tolerances.linear = cadmpeg_ir::scalar::PositiveLength::new(linear).unwrap();
        }
        let (losses, written) = write_without_source(&ir);
        assert!(
            !losses
                .iter()
                .any(|loss| loss.code.local_code() == "value.unwritable"
                    && loss.message.contains("tolerances.linear")),
            "{losses:#?}"
        );
        assert_eq!(decode(written).ir().tolerances.linear, ir.tolerances.linear);
    }
}

/// A one-face body whose surface reference names no carrier, so the decode
/// holds an untyped surface and a write of an edit patches its partition.
fn untyped_surface_triangle() -> Vec<u8> {
    use crate::test_support::parasolid::{
        bridge, coedge, complete_fixture_vertex_owners, edge_use, loop_head, vertex_use,
        world_point,
    };
    let mut body = bridge(10, 20, 999);
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    for edge in [40, 41, 42] {
        body.extend(edge_use(edge, 0));
    }
    for (vertex, point) in [(50, 60), (51, 61), (52, 62)] {
        body.extend(vertex_use(vertex, point));
    }
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    complete_fixture_vertex_owners(&mut body);
    body
}

/// An edit of the active configuration beside a configuration whose bodies
/// store a coarser `res_linear` patches the active partition in place and
/// keeps the other partition.
#[test]
fn an_edit_beside_a_coarser_configuration_patches_the_active_partition() {
    let first = parasolid_with_body(
        "partition body",
        "SCH_SW_33103_11000",
        &owned_triangle_with_resolution(0, 700, 0.0, 1.0e-6),
    );
    let second = parasolid_with_body(
        "partition body",
        "SCH_SW_33103_11000",
        &untyped_surface_triangle(),
    );
    let mut partitions = outer_header();
    partitions.extend(make_block(0x20, "Contents/Config-0-Partition", &first));
    partitions.extend(make_block(0x21, "Contents/Config-1-Partition", &second));
    let mut result = decode(two_configurations_of(partitions));
    assert_eq!(
        Some(result.ir().tolerances.linear),
        cadmpeg_parasolid::precision::linear_precision(1.0e-6)
    );
    {
        let mut ir = result.ir_mut();
        let point = &mut ir.geometry.points[1];
        let at = point.position().get();
        point.set_position(
            cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                at.x + 0.25,
                at.y,
                at.z,
            ))
            .unwrap(),
        );
    }
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        result.ir(),
        result.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let payload = |name: &str| {
        scan.blocks
            .iter()
            .find(|block| block.section.name() == Some(name))
            .map_or_else(|| panic!("{name}"), |block| block.payload.clone())
    };
    assert_eq!(payload("Contents/Config-0-Partition"), first);
    let active = payload("Contents/Config-1-Partition");
    assert_ne!(active, second);
    assert_eq!(active.len(), second.len());
}

/// `ir` with its current bodies, its stored bodies, or both made wire bodies,
/// which the partition encoder cannot write as a solid lump.
fn with_wire_bodies(ir: &cadmpeg_ir::CadIr, current: bool, stored: bool) -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::topology::BodyKind;
    let mut ir = ir.clone();
    if current {
        for body in &mut ir.geometry.bodies {
            body.kind = BodyKind::Wire;
        }
    }
    if stored {
        for content in ir
            .design
            .evaluations
            .iter_mut()
            .filter_map(|evaluation| evaluation.content.as_mut())
        {
            for body in &mut content.delta.records.bodies {
                body.kind = BodyKind::Wire;
            }
        }
    }
    ir
}

/// The message of the write's note on the bodies it cannot encode.
fn bodies_omitted(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Option<&str> {
    losses
        .iter()
        .find(|loss| loss.code.local_code() == "writer.bodies-omitted")
        .map(|loss| loss.message.as_str())
}

/// A current body the encoder cannot write is left out with a note, and the
/// stored state of the other configuration is still written.
#[test]
fn a_current_body_the_encoder_cannot_write_is_left_out_with_a_note() {
    let result = decode(two_configurations());
    let ir = with_wire_bodies(result.ir(), true, false);
    let (losses, written) = write_without_source(&ir);
    let note = bodies_omitted(&losses).unwrap_or_else(|| panic!("{losses:#?}"));
    assert!(note.contains(ir.geometry.bodies[0].id.as_str()), "{note}");
    let reread = decode(written);
    assert_eq!(stored_states(reread.ir()), stored_states(&ir));
}

/// A body of a stored state that the encoder cannot write is left out and
/// named with its configuration; the rest of the part is written. Through
/// the configuration partitions, and through the partitions of a part whose
/// current geometry holds no face.
#[test]
fn a_stored_body_the_encoder_cannot_write_is_left_out_with_its_configuration() {
    for source in [two_configurations(), faceless_active_configuration(true)] {
        let result = decode(source);
        let ir = with_wire_bodies(result.ir(), false, true);
        let first = ir
            .model
            .configurations
            .iter()
            .find(|configuration| configuration.name.as_deref() == Some("First"))
            .unwrap();
        let stored = ir
            .design
            .evaluations
            .iter()
            .find_map(|evaluation| evaluation.content.as_ref())
            .unwrap();
        let [body] = stored.delta.records.bodies.as_slice() else {
            panic!("one stored body");
        };
        let (losses, written) = write_without_source(&ir);
        let note = bodies_omitted(&losses).unwrap_or_else(|| panic!("{losses:#?}"));
        assert!(note.contains(body.id.as_str()), "{note}");
        assert!(note.contains(first.id.as_str()), "{note}");
        let bodiless = bodiless_note(&losses).unwrap_or_else(|| panic!("{losses:#?}"));
        assert!(
            bodiless.contains(&format!(
                "Contents/Config-{}-Partition",
                first.source_index.unwrap()
            )),
            "{bodiless}"
        );
        let reread = decode(written);
        assert_eq!(reread.ir().geometry.bodies.len(), ir.geometry.bodies.len());
        assert!(stored_states(reread.ir())
            .iter()
            .all(|(bodies, _)| *bodies == 0));
    }
}

/// Two configurations: `First`, not active, whose partition holds a triangle
/// whose face names `Boss` as its producer in `FACE_ID_2001`, and, with
/// `modified`, whose body names `Cut` as the last feature that modified it;
/// and `Second`, active, with a triangle of its own whose face names `Boss`.
/// Both lanes list `Boss` and `Cut`.
fn stored_producers(modified: bool) -> Vec<u8> {
    use crate::test_support::history::resolved_feature_classes_with_ids;
    use crate::test_support::parasolid::{int_list_attribute, triangle_body};
    let mut stored = triangle_body();
    stored.extend(int_list_attribute(
        "FACE_ID_2001",
        &[9, 9, 1, 1],
        1100,
        10,
        &[36, 7, 0x6523_4178, 0],
    ));
    if modified {
        stored.extend(int_list_attribute(
            "LAST_BODY_MODIFYING_FEATURE_ID",
            &[9, 1],
            1120,
            900,
            &[9],
        ));
    }
    let mut active = triangle_body();
    active.extend(int_list_attribute(
        "FACE_ID_2001",
        &[9, 9, 1, 1],
        1100,
        10,
        &[36, 7, 0x6523_4179, 0],
    ));
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &stored),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &active),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket">
    <Configuration Name="First" SourceIndex="0"/>
    <Configuration Name="Second" SourceIndex="1"/>
    <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
    <Extrusion Name="Cut" Type="CutExtrude" id="9"/>
</Keywords>"#,
    ));
    for lane in 0..2 {
        source.extend(make_block(
            0x44 + lane,
            &format!("Contents/Config-{lane}-ResolvedFeatures"),
            &resolved_feature_classes_with_ids(&[
                ("moExtrusion_c", "Boss", 7),
                ("moExtrusion_c", "Cut", 9),
            ]),
        ));
    }
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));
    source
}

fn outputs_unstated(result: &cadmpeg_test_support::EditableDecodeResult) -> Vec<&str> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::ConfigStateOutputsUnstated.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// An output of a stored state whose feature a face of the body names in
/// the configuration's evaluation is stated there, and charges no loss.
#[test]
fn a_stored_output_a_face_name_states_charges_no_loss() {
    let result = decode(stored_producers(false));
    assert!(matches!(stored_states(result.ir()).as_slice(), [(1, 1)]));
    assert!(
        outputs_unstated(&result).is_empty(),
        "{:#?}",
        result.report().losses
    );
}

/// An output of a stored state that only the last-modifying-feature
/// attribute gives, for a feature that names no face of the body, is
/// charged, once for its configuration.
#[test]
fn a_stored_output_no_face_name_states_is_charged() {
    let result = decode(stored_producers(true));
    let configuration = result
        .ir()
        .design
        .evaluations
        .iter()
        .find_map(|evaluation| {
            evaluation
                .content
                .as_ref()
                .and(evaluation.configuration.as_ref())
        })
        .unwrap();
    let [message] = outputs_unstated(&result)[..] else {
        panic!("one loss: {:#?}", result.report().losses);
    };
    assert!(message.contains(configuration.as_str()), "{message}");
    assert!(message.contains(": 1 feature output(s)"), "{message}");
}

/// The document feature outputs, configuration feature-state outputs, and
/// face and body names of the current state and the stored states of `ir`.
fn producers_and_names(ir: &cadmpeg_ir::CadIr) -> [usize; 4] {
    use cadmpeg_ir::design::refs::EntityRef;
    let feature_outputs = ir
        .model
        .features
        .iter()
        .map(|feature| feature.evaluation.outputs().len())
        .sum();
    let state_outputs = ir
        .model
        .configurations
        .iter()
        .flat_map(|configuration| configuration.feature_states.values())
        .map(|state| state.evaluation.outputs().len())
        .sum();
    let names = ir.design.names.entries().keys().chain(
        ir.design
            .evaluations
            .iter()
            .filter_map(|evaluation| evaluation.content.as_ref())
            .flat_map(|content| content.delta.names.iter())
            .filter(|(_, entry)| entry.is_some())
            .map(|(entity, _)| entity),
    );
    let (mut faces, mut bodies) = (0, 0);
    for entity in names {
        match entity {
            EntityRef::Face(_) => faces += 1,
            EntityRef::Body(_) => bodies += 1,
            _ => {}
        }
    }
    [feature_outputs, state_outputs, faces, bodies]
}

/// A write with no retained source generates partitions without the
/// attributes the feature outputs and the face and body names are read from,
/// and reports that it could not tell the part's edits.
/// Its note counts what the document states of them, and the written part
/// reads back with none of them.
#[test]
fn a_part_written_without_its_source_counts_the_outputs_and_names_it_drops() {
    let result = decode(stored_producers(true));
    let ir = result.ir();
    let [feature_outputs, state_outputs, faces, bodies] = producers_and_names(ir);
    assert!(feature_outputs > 0 && state_outputs > 0 && faces > 0);
    let (losses, written) = write_without_source(ir);
    let note = losses
        .iter()
        .find(|loss| loss.code.local_code() == "writer.producer-attributes-omitted")
        .unwrap_or_else(|| panic!("{losses:#?}"));
    // With no sidecar the writer cannot tell the part's edits, reads it as
    // unedited, and says so.
    assert!(
        losses
            .iter()
            .any(|loss| loss.code.namespace() == "export"
                && loss.code.local_code() == "edit.unverified"),
        "{losses:#?}"
    );
    let counts = format!(
        "{feature_outputs} document feature output(s), {state_outputs} configuration \
         feature-state output(s), {faces} face name(s) and {bodies} body name(s)"
    );
    assert!(note.message.contains(&counts), "{}", note.message);
    let reread = decode(written);
    assert_eq!(producers_and_names(reread.ir()), [0; 4]);
}

/// A write that keeps the source's partition writes no generated partition,
/// and charges no such note.
#[test]
fn a_part_written_with_its_partition_charges_no_producer_note() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let result = decode(native_envelope_fixture(&stored_producers(true)));
    let mut written = Vec::new();
    let report = crate::SldprtCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Unnamed,
        )
        .unwrap()
        .write_to(&mut written)
        .unwrap();
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "writer.producer-attributes-omitted"),
        "{:#?}",
        report.losses
    );
}
