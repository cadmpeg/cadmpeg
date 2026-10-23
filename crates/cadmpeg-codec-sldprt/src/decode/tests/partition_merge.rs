// SPDX-License-Identifier: Apache-2.0
//! Partition/deltas merge, override, and topology-recovery decode tests.

#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::container::sldprt_with_partition_and_deltas;
use crate::test_support::parasolid::bridge;
use crate::test_support::parasolid::bridge_owned;
use crate::test_support::parasolid::entity51;
use crate::test_support::parasolid::entity53_color;
use crate::test_support::parasolid::face_color_definition;
use crate::test_support::parasolid::framed_topology_fin;
use crate::test_support::parasolid::framed_topology_loop;
use crate::test_support::parasolid::owned_triangle;
use crate::test_support::parasolid::parasolid_with_body;
use crate::test_support::parasolid::prefixed_edge_triangle_body;
use crate::test_support::parasolid::suffix_prefixed_edge_triangle_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::parasolid::triangle_body_with_overlapping_point;
use crate::test_support::parasolid::tripled_coedge;
use crate::test_support::parasolid::tripled_triangle_body;
use crate::test_support::parasolid::tripled_world_point;
use crate::test_support::parasolid::untyped_triangle;
use crate::test_support::parasolid::world_point;
use crate::test_support::parasolid::FACE_COLOR_DEFINITION_ID;
use crate::SldprtCodec;

#[test]
fn decode_merges_partition_and_deltas_records() {
    let mut partition = triangle_body();
    let missing_point = world_point(60, [0.0, 0.0, 0.0]);
    let point_start = partition
        .windows(missing_point.len())
        .position(|window| window == missing_point.as_slice())
        .expect("partition point record");
    drop(partition.drain(point_start..point_start + missing_point.len()));
    let deltas = tripled_world_point(60, [2.0, 0.0, 0.0]);
    let f = sldprt_with_partition_and_deltas(&partition, &deltas);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get().x == 2000.0));
}

#[test]
fn conflicting_missing_partition_point_has_one_counted_loss() {
    let mut partition = triangle_body();
    let missing = world_point(60, [0.0, 0.0, 0.0]);
    let at = partition
        .windows(missing.len())
        .position(|window| window == missing)
        .unwrap();
    drop(partition.drain(at..at + missing.len()));
    let first = tripled_world_point(60, [2.0, 0.0, 0.0]);
    let second = tripled_world_point(60, [3.0, 0.0, 0.0]);
    for metadata_fallback in [false, true] {
        let mut partition = partition.clone();
        if metadata_fallback {
            partition.extend(entity51(1, 711, 712, &[10, 1, 1, 1, 1, 1]));
        }
        for records in [[&first, &second, &first], [&second, &first, &second]] {
            let deltas: Vec<u8> = records.into_iter().flatten().copied().collect();
            let result = SldprtCodec
                .decode(
                    &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            assert!(!result.ir().model.points.iter().any(|point| {
                let x = point.position().get().x;
                x == 2000.0 || x == 3000.0
            }));
            let losses: Vec<_> = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "geometry.point-record-ambiguous")
                .collect();
            assert_eq!(losses.len(), 1);
            assert!(losses[0].message.starts_with("1 referenced POINT"));
            if metadata_fallback {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "topology.graph-not-transferred"));
            }
        }
    }
}

#[test]
fn unused_history_point_conflict_has_no_geometry_loss() {
    let deltas = [
        tripled_world_point(211, [2.0, 0.0, 0.0]),
        tripled_world_point(211, [3.0, 0.0, 0.0]),
    ]
    .concat();
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&triangle_body(), &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "geometry.point-record-ambiguous"));
}

fn partition_missing_loop() -> Vec<u8> {
    let mut partition = triangle_body();
    let original = crate::test_support::parasolid::loop_head(20, 30, 10);
    let at = partition
        .windows(original.len())
        .position(|window| window == original)
        .unwrap();
    drop(partition.drain(at..at + original.len()));
    partition
}

fn partition_missing_middle_fin() -> Vec<u8> {
    let mut partition = triangle_body();
    let original = crate::test_support::parasolid::coedge(31, 20, 32, 51, 0, 41, false);
    let at = partition
        .windows(original.len())
        .position(|window| window == original)
        .unwrap();
    drop(partition.drain(at..at + original.len()));
    partition
}

#[test]
fn missing_fin_closes_through_partition_loop_and_ring_context() {
    let fin = framed_topology_fin(31, [0, 20, 0, 32, 51, 0, 41, 0, 0], true, true, false);
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(
                &partition_missing_middle_fin(),
                &fin,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.fin-record-ambiguous"));
}

#[test]
fn conflicting_fin_framing_keeps_one_loss_through_empty_and_metadata_paths() {
    let refs = [0, 20, 0, 32, 51, 0, 41, 0, 0];
    let first = framed_topology_fin(31, refs, true, false, false);
    let second = framed_topology_fin(31, refs, true, true, false);
    for metadata_fallback in [false, true] {
        let mut partition = partition_missing_middle_fin();
        if metadata_fallback {
            partition.extend(entity51(1, 711, 712, &[10, 1, 1, 1, 1, 1]));
        }
        for records in [[&first, &second, &first], [&second, &first, &second]] {
            let deltas: Vec<u8> = records.into_iter().flatten().copied().collect();
            let result = SldprtCodec
                .decode(
                    &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            assert!(result.ir().model.faces.is_empty());
            let losses: Vec<_> = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "topology.fin-record-ambiguous")
                .collect();
            assert_eq!(losses.len(), 1);
            assert!(losses[0].message.starts_with("1 referenced FIN"));
            if metadata_fallback {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "topology.graph-not-transferred"));
            }
        }
    }
}

#[test]
fn conflicting_first_partition_fin_retains_loop_reference_for_loss() {
    let mut partition = triangle_body();
    partition.extend(framed_topology_fin(
        30,
        [0, 20, 0, 31, 50, 0, 40, 0, 0],
        false,
        true,
        false,
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &[])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.faces.is_empty());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "topology.fin-record-ambiguous")
            .count(),
        1
    );
}

#[test]
fn unused_history_fin_conflict_has_no_topology_loss() {
    let refs = [0, 21, 0, 33, 50, 0, 43, 0, 0];
    let deltas = [
        framed_topology_fin(33, refs, true, false, false),
        framed_topology_fin(33, refs, true, true, false),
    ]
    .concat();
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&triangle_body(), &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.fin-record-ambiguous"));
}

#[test]
fn missing_loop_closes_against_partition_face_and_fin_before_history() {
    let mut deltas = framed_topology_loop(20, 811, [0, 30, 10, 0], true, false);
    deltas.extend(tripled_coedge(30, 77, 31, 50, 40));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(
                &partition_missing_loop(),
                &deltas,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.loop-record-ambiguous"));
}

#[test]
fn conflicting_loop_node_ids_keep_one_loss_through_empty_and_metadata_paths() {
    let first = framed_topology_loop(20, 811, [0, 30, 10, 0], true, false);
    let second = framed_topology_loop(20, 812, [0, 30, 10, 0], true, false);
    for metadata_fallback in [false, true] {
        let mut partition = partition_missing_loop();
        if metadata_fallback {
            partition.extend(entity51(1, 711, 712, &[10, 1, 1, 1, 1, 1]));
        }
        for records in [[&first, &second, &first], [&second, &first, &second]] {
            let deltas: Vec<u8> = records.into_iter().flatten().copied().collect();
            let result = SldprtCodec
                .decode(
                    &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            assert!(result.ir().model.loops.is_empty());
            let losses: Vec<_> = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "topology.loop-record-ambiguous")
                .collect();
            assert_eq!(losses.len(), 1);
            assert!(losses[0].message.starts_with("1 referenced LOOP"));
            if metadata_fallback {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "topology.graph-not-transferred"));
            }
        }
    }
}

#[test]
fn unused_history_loop_conflict_has_no_topology_loss() {
    let mut deltas = [
        framed_topology_loop(21, 811, [0, 33, 10, 0], true, false),
        framed_topology_loop(21, 812, [0, 33, 10, 0], true, false),
    ]
    .concat();
    deltas.extend(tripled_coedge(33, 21, 33, 50, 43));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&triangle_body(), &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "topology.loop-record-ambiguous"));
}

#[test]
fn typed_ownership_can_close_across_partition_and_deltas() {
    let full = triangle_body();
    let typed_start = full
        .windows(3)
        .position(|window| window == [0x00, 0x0c, 0xff])
        .expect("typed body");
    let partition = &full[..typed_start];
    let deltas = &full[typed_start..];
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(partition, deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.shells[0].faces().len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("No body record was available")));
}

#[test]
fn decode_deduplicates_partition_and_deltas_face_bindings() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut partition = Vec::new();
    partition.extend(face_color_definition());
    partition.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    partition.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    partition.extend(owned_triangle(0, 700, 0.0));
    let mut deltas = Vec::new();
    deltas.extend(face_color_definition());
    deltas.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    deltas.extend(entity53_color(900, [0.25, 0.5, 0.75]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn merged_face_color_annotations_retain_site_owners() {
    let mut first = face_color_definition();
    first.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    first.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    first.extend(owned_triangle(0, 700, 0.0));
    let mut second = face_color_definition();
    second.extend(entity51(
        1,
        701,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 901],
    ));
    second.extend(entity53_color(901, [0.75, 0.5, 0.25]));
    second.extend(owned_triangle(0, 701, 10.0));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("first partition", "SCH_SW_33103_11000", &first),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("second partition", "SCH_SW_33103_11000", &second),
    ));

    let result = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap(),
    );
    let provenance = &result.source_fidelity().annotations.provenance;
    let first = provenance
        .iter()
        .find(|(id, _)| id.starts_with("sldprt:appearance:entity53#900@"))
        .expect("first merged face color provenance");
    assert_eq!(first.1.stream(), "Contents/Config-0-Partition");
    let second = provenance
        .iter()
        .find(|(id, _)| id.starts_with("sldprt:appearance:entity53#901@"))
        .expect("second merged face color provenance");
    assert_eq!(second.1.stream(), "Contents/Config-1-Partition");
}

#[test]
fn merged_face_colors_keep_same_site_local_attributes_distinct() {
    let mut first = face_color_definition();
    first.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    first.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    first.extend(owned_triangle(0, 700, 0.0));
    let mut second = face_color_definition();
    second.extend(entity51(
        1,
        701,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    second.extend(entity53_color(900, [0.75, 0.5, 0.25]));
    second.extend(owned_triangle(0, 701, 10.0));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("first partition", "SCH_SW_33103_11000", &first),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("second partition", "SCH_SW_33103_11000", &second),
    ));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let colors = result
        .ir()
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.schema.as_deref() == Some("entity-53"))
        .map(|appearance| appearance.base_color.expect("entity-53 color"))
        .collect::<Vec<_>>();
    assert_eq!(colors.len(), 2);
    assert!(colors.contains(&cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).unwrap()));
    assert!(colors.contains(&cadmpeg_ir::topology::Color::new(0.75, 0.5, 0.25, 1.0).unwrap()));
}

#[test]
fn merged_unbound_face_colors_keep_site_local_attributes_distinct() {
    let mut first = face_color_definition();
    first.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    first.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    first.extend(entity51(
        1,
        701,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 901],
    ));
    first.extend(entity53_color(901, [0.1, 0.2, 0.3]));
    first.extend(owned_triangle(0, 700, 0.0));
    let mut second = face_color_definition();
    second.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    second.extend(entity53_color(900, [0.75, 0.5, 0.25]));
    second.extend(entity51(
        1,
        701,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 901],
    ));
    second.extend(entity53_color(901, [0.9, 0.8, 0.7]));
    second.extend(owned_triangle(0, 700, 10.0));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("first partition", "SCH_SW_33103_11000", &first),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body("second partition", "SCH_SW_33103_11000", &second),
    ));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let unbound = result
        .ir()
        .model
        .appearances
        .iter()
        .filter(|appearance| {
            appearance.schema.as_deref() == Some("entity-53")
                && appearance.id.as_str().contains("#901@")
        })
        .map(|appearance| appearance.base_color.expect("entity-53 color"))
        .collect::<Vec<_>>();
    assert_eq!(unbound.len(), 2);
    assert!(unbound.contains(&cadmpeg_ir::topology::Color::new(0.1, 0.2, 0.3, 1.0).unwrap()));
    assert!(unbound.contains(&cadmpeg_ir::topology::Color::new(0.9, 0.8, 0.7, 1.0).unwrap()));
}

#[test]
fn merged_opaque_geometry_retains_its_owning_site() {
    use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &untyped_triangle(0.0),
        ),
    ));
    source.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &untyped_triangle(10.0),
        ),
    ));
    let expected_records = crate::test_support::container::scan(&source)
        .blocks
        .iter()
        .map(|block| {
            cadmpeg_ir::ids::UnknownId::mint(format!("sldprt:file:block#{}", block.offset))
                .expect("identity grammar")
        })
        .collect::<std::collections::BTreeSet<_>>();

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    let surface_bindings = result
        .ir()
        .model
        .surfaces
        .iter()
        .map(|surface| {
            let Some(SolvedSurfaceGeometry::Unknown {
                record: Some(record),
            }) = surface.geometry.solved()
            else {
                panic!("site surface is not bound to opaque source bytes");
            };
            (surface.id.as_str().to_owned(), record.clone())
        })
        .collect::<Vec<_>>();
    let curve_bindings = result
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| {
            let Some(SolvedCurveGeometry::Unknown {
                record: Some(record),
            }) = curve.geometry.solved()
            else {
                panic!("site curve is not bound to opaque source bytes");
            };
            (curve.id.as_str().to_owned(), record.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(surface_bindings.len(), 2);
    assert_eq!(curve_bindings.len(), 2);
    assert_eq!(
        surface_bindings
            .iter()
            .map(|(_, record)| record.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_records
    );
    assert_eq!(
        curve_bindings
            .iter()
            .map(|(_, record)| record.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected_records
    );
    let unknowns = result.ir().native_unknowns("sldprt").unwrap();
    for (geometry, record) in surface_bindings.into_iter().chain(curve_bindings) {
        assert!(unknowns
            .iter()
            .find(|unknown| unknown.id == record)
            .is_some_and(|unknown| { unknown.links.iter().any(|link| link.as_str() == geometry) }));
    }
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn partition_point_coordinates_win_over_complete_deltas_records() {
    let partition = triangle_body();
    let deltas = world_point(60, [2.0, 0.0, 0.0]);
    let f = sldprt_with_partition_and_deltas(&partition, &deltas);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str().ends_with("#60"))
        .expect("partition point");

    assert_eq!(point.position().get().x, 0.0);
}

#[test]
fn partition_topology_wins_when_deltas_reuse_a_bridge_identity() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let partition = triangle_body();
    let deltas = bridge_owned(10, 120, 200, 700);
    let partition_payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &partition);
    let deltas_payload = parasolid_with_body("deltas body", "SCH_SW_33103_11000", &deltas);
    let partition_header = crate::parasolid::stream_header(&partition_payload)
        .unwrap()
        .unwrap();
    let deltas_header = crate::parasolid::stream_header(&deltas_payload)
        .unwrap()
        .unwrap();

    let decoded = crate::brep::graph::decode_bodies(
        &ctx,
        &[
            (&deltas_payload, &deltas_header),
            (&partition_payload, &partition_header),
        ],
        &cadmpeg_ir::stream_name!("precedence"),
    )
    .expect("valid exactness fields");

    assert_eq!(decoded.faces.len(), 1);
    assert_eq!(decoded.faces[0].id.as_str(), "sldprt:brep:face#10");
    assert_eq!(decoded.faces[0].surface.as_str(), "sldprt:brep:surf#10");
}

#[test]
fn unselected_deltas_bridges_do_not_enter_partition_membership() {
    let partition = triangle_body();
    let deltas = bridge(210, 220, 300);
    let mut cur = Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas));

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.position().get().x != 10_000.0));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn partition_point_refs_do_not_select_deltas_framing() {
    let mut body = triangle_body();
    let point = body
        .windows(4)
        .position(|window| window == [0x00, 0x1d, 0x00, 0x3c])
        .expect("point 60");
    for (index, reference) in [1u16, 378, 379, 373].into_iter().enumerate() {
        let at = point + 8 + index * 2;
        body[at..at + 2].copy_from_slice(&reference.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn deltas_point_index_does_not_replace_partition_coordinates() {
    let partition = triangle_body();
    let mut deltas = Vec::new();
    for attr in 60u16..80 {
        deltas.extend_from_slice(&[0x00, 0x1d]);
        deltas.extend_from_slice(&attr.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str().ends_with("#60"))
        .unwrap();
    assert_eq!(
        point.position().get(),
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_recovers_overlapping_topology_records() {
    let f = sldprt_with_body(&triangle_body_with_overlapping_point());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
}

#[test]
fn decode_recovers_tripled_deltas_topology() {
    let mut cur = Cursor::new(sldprt_with_body(&tripled_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.faces.len(), 1);
}

#[test]
fn decode_resolves_prefixed_deltas_edge_curve() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let mut cur = Cursor::new(sldprt_with_body(&prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    )));
}

#[test]
fn decode_resolves_suffix_prefixed_edge_curve_with_high_byte_one() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let mut cur = Cursor::new(sldprt_with_body(&suffix_prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    )));
}
