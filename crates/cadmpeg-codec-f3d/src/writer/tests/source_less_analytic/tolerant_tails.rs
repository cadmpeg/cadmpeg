// SPDX-License-Identifier: Apache-2.0
//! Tolerant edge and vertex tails through a write and decode cycle.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::finite_tolerance_pair;
use crate::test_support::native_test::{f3d_native, f3d_native_mut, TestEncode};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn tolerant_edge_and_vertex_tails_round_trip_all_trailing_forms() {
    // The tedge tail carries the serializer revision stamp, then a trailing
    // LONG present only in newer streams and taking the values 0, 1, and 2.
    // The tvertex trailing LONG shares the gate and the value set. Each form
    // must survive a write/decode cycle byte-for-byte.
    for (edge_trailing, vertex_trailing) in [
        (Some(0), Some(0)),
        (Some(1), Some(1)),
        (Some(2), Some(2)),
        (None, None),
    ] {
        let source = f3d_with_smbh(&synthetic_geometry_smbh());
        let decoded = F3dCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("generated planar triangle decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                "f3d",
                &[],
            )
            .unwrap();
        source_less.geometry.vertices[0].tolerance = Some(
            cadmpeg_ir::scalar::PositiveLength::new(0.025).expect("positive finite tolerance"),
        );
        source_less.geometry.edges[0].tolerance = Some(
            cadmpeg_ir::scalar::PositiveLength::new(0.035).expect("positive finite tolerance"),
        );
        let tolerant_vertex = source_less.geometry.vertices[0].id.clone();
        let tolerant_edge = source_less.geometry.edges[0].id.clone();
        {
            let mut native = f3d_native_mut(&mut source_less);
            native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                vertex: tolerant_vertex,
                record_index: 0,
                leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
                evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Evaluated {
                    trailing: vertex_trailing,
                },
            }];
            native.tolerant_edge_tails = vec![cadmpeg_asm::brep::records::TolerantEdgeTail {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                edge: tolerant_edge,
                record_index: 0,
                entity_revision: 22800,
                trailing_field: edge_trailing,
            }];
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("tolerant tail encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("tolerant tail round trip");
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].entity_revision,
            22800
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].trailing_field,
            edge_trailing
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_vertex_tails[0]
                .evaluated_slot
                .trailing(),
            vertex_trailing
        );
    }
}

#[test]
fn an_unset_tolerant_vertex_sentinel_round_trips_without_a_neutral_tolerance() {
    // The `-1` unset evaluated slot is a marker rather than a length: the
    // neutral vertex carries no tolerance, the native tail keeps the unset
    // fact, and generation writes the sentinel back into a tvertex record.
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let tolerant_vertex = source_less.geometry.vertices[0].id.clone();
    assert_eq!(source_less.geometry.vertices[0].tolerance, None);
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Unset {
                trailing: Some(0),
            },
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("unset tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("unset tolerant vertex round trip");
    let vertex = round_trip
        .ir()
        .geometry
        .vertices
        .iter()
        .find(|vertex| {
            f3d_native(round_trip.ir())
                .tolerant_vertex_tails
                .iter()
                .any(|tail| tail.vertex == vertex.id)
        })
        .expect("tolerant vertex survives");
    assert_eq!(vertex.tolerance, None);
    let tail = &f3d_native(round_trip.ir()).tolerant_vertex_tails[0];
    assert_eq!(
        tail.evaluated_slot,
        cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Unset { trailing: Some(0) }
    );
    assert_eq!(
        tail.leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [-1.0, -1.0]
    );
}
#[test]
fn an_absent_tolerant_vertex_slot_round_trips_without_a_neutral_tolerance() {
    // A tvertex record that ends before the evaluated slot has no neutral
    // tolerance; the native tail keeps the absent fact and generation writes
    // only the leading slots back.
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let tolerant_vertex = source_less.geometry.vertices[0].id.clone();
    assert_eq!(source_less.geometry.vertices[0].tolerance, None);
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Absent {},
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("absent tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("absent tolerant vertex round trip");
    let vertex = round_trip
        .ir()
        .geometry
        .vertices
        .iter()
        .find(|vertex| {
            f3d_native(round_trip.ir())
                .tolerant_vertex_tails
                .iter()
                .any(|tail| tail.vertex == vertex.id)
        })
        .expect("tolerant vertex survives");
    assert_eq!(vertex.tolerance, None);
    let tail = &f3d_native(round_trip.ir()).tolerant_vertex_tails[0];
    assert_eq!(
        tail.evaluated_slot,
        cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Absent {}
    );
    assert_eq!(tail.evaluated_slot.trailing(), None);
    assert_eq!(
        tail.leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [-1.0, -1.0]
    );
}
