// SPDX-License-Identifier: Apache-2.0
//! Partition/deltas merge, override, and topology-recovery decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_merges_partition_and_deltas_records() {
    let partition = triangle_body();
    let deltas = world_point(60, [2.0, 0.0, 0.0]);
    let f = sldprt_with_partition_and_deltas(&partition, &deltas);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position.x == 2000.0));
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
    assert_eq!(result.ir().model.shells[0].faces.len(), 1);
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
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn merged_opaque_geometry_retains_its_owning_site() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

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
    let expected_records = container::scan_bytes(&source)
        .blocks
        .iter()
        .map(|block| cadmpeg_ir::ids::UnknownId(format!("sldprt:file:block#{}", block.offset)))
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
            let SurfaceGeometry::Unknown {
                record: Some(record),
            } = &surface.geometry
            else {
                panic!("site surface is not bound to opaque source bytes");
            };
            (surface.id.0.clone(), record.clone())
        })
        .collect::<Vec<_>>();
    let curve_bindings = result
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| {
            let CurveGeometry::Unknown {
                record: Some(record),
            } = &curve.geometry
            else {
                panic!("site curve is not bound to opaque source bytes");
            };
            (curve.id.0.clone(), record.clone())
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
            .is_some_and(|unknown| unknown.links.contains(&geometry)));
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn deltas_full_record_overrides_partition_record() {
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
        .find(|point| point.id.0.ends_with("#60"))
        .expect("overridden point");

    assert_eq!(point.position.x, 2000.0);
}

#[test]
fn partition_topology_wins_when_deltas_reuse_a_bridge_identity() {
    let partition = triangle_body();
    let deltas = bridge_owned(10, 120, 200, 700);
    let partition_payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &partition);
    let deltas_payload = parasolid_with_body("deltas body", "SCH_SW_33103_11000", &deltas);
    let partition_header = crate::parasolid::stream_header(&partition_payload).unwrap();
    let deltas_header = crate::parasolid::stream_header(&deltas_payload).unwrap();

    let decoded = crate::brep::decode_bodies(
        &[
            (&deltas_payload, &deltas_header),
            (&partition_payload, &partition_header),
        ],
        "precedence",
    );

    assert_eq!(decoded.faces.len(), 1);
    assert_eq!(decoded.faces[0].id.0, "sldprt:brep:face#10");
    assert_eq!(decoded.faces[0].surface.0, "sldprt:brep:surf#10");
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
        .all(|point| point.position.x != 10_000.0));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
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
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
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
        .find(|point| point.id.0.ends_with("#60"))
        .unwrap();
    assert_eq!(point.position, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
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
    use cadmpeg_ir::geometry::CurveGeometry;
    let mut cur = Cursor::new(sldprt_with_body(&prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn decode_resolves_suffix_prefixed_edge_curve_with_high_byte_one() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let mut cur = Cursor::new(sldprt_with_body(&suffix_prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}
