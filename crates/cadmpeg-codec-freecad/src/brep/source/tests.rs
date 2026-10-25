// SPDX-License-Identifier: Apache-2.0
//! Exact sequential extents and bounded shape-set recovery.

use super::*;
mod partial_polygons;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

const OWNER: &str = "fcstd:native:shape-payload#fixture";

fn decode<T>(
    bytes: &[u8],
    binary: bool,
    inspect: impl FnOnce(&DecodeContext<'_>, &PartOutput, Option<ShapePayload>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let payload = ctx
        .decode_part(
            &mut output,
            source,
            spec(
                if binary {
                    "freecad.brep_binary"
                } else {
                    "freecad.brep_text"
                },
                vec![],
            )
            .unwrap(),
            |stage| {
                let payload = if binary {
                    binary_shape(&ctx, source, OWNER, stage)?
                        .map(|(facts, version)| ShapePayload::Binary { facts, version })
                } else {
                    text_shape(&ctx, source, OWNER, stage)?
                        .map(|(facts, version)| ShapePayload::Text { facts, version })
                };
                if let Some(payload) = &payload {
                    let record = records::with_rows(
                        &ctx,
                        stage.native(),
                        ShapePayloadRecord {
                            id: OWNER.to_owned(),
                            property: "fcstd:native:property#fixture".to_owned(),
                            entry: "fcstd:native:entry#fixture".to_owned(),
                            rows: Vec::new(),
                            payload: payload.clone(),
                        },
                    )
                    .unwrap();
                    stage.insert_native(&ctx, "shape_payloads", record.clone())?;
                }
                Ok(payload)
            },
        )
        .unwrap()
        .flatten();
    inspect(&ctx, &output, payload)
}

fn table_rows(output: &PartOutput) -> usize {
    output
        .native()
        .namespace("fcstd")
        .and_then(|namespace| {
            namespace
                .arena_as::<ShapePayloadRecord>("shape_payloads")
                .unwrap()
                .into_iter()
                .next()
        })
        .and_then(|payload| {
            payload.payload.shape_set().map(|facts| {
                facts.locations.len()
                    + facts.curve2ds.len()
                    + facts.curves.len()
                    + facts.surfaces.len()
                    + facts.triangulations.len()
                    + facts.tshapes.len()
            })
        })
        .unwrap_or(0)
}

fn empty_binary() -> Vec<u8> {
    b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n".to_vec()
}

#[test]
fn zero_count_binary_tables_create_no_record_attempts_or_synthetic_parts() {
    decode(&empty_binary(), true, |ctx, output, payload| {
        assert!(output.losses().is_empty());
        assert!(table_rows(output) == 0);
        assert!(payload.unwrap().shape_set().unwrap().tshapes.is_empty());
        let parts = output.fidelity().parts().values().collect::<Vec<_>>();
        assert_eq!(parts.len(), 10);
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_binary"));
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_binary_header"));
        let markers = parts
            .iter()
            .filter(|part| part.level().as_str() == "freecad.brep_binary_record")
            .collect::<Vec<_>>();
        assert_eq!(markers.len(), TABLES.len());
        let bytes = empty_binary();
        let actual = markers
            .iter()
            .map(|part| {
                let start = usize::try_from(part.offset()).unwrap();
                let end = start + usize::try_from(part.byte_len()).unwrap();
                assert!(matches!(part.state(), PartState::Decoded {}));
                std::str::from_utf8(&bytes[start..end]).unwrap().to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        let expected = TABLES
            .map(|name| format!("{name} 0\n"))
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected);
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 2);
        assert!(captures.iter().any(|capture| capture.id() == OWNER));
        assert!(captures
            .iter()
            .any(|capture| capture.id() == census_id(OWNER)));
    });
}

#[test]
fn malformed_uncovered_binary_selector_gives_up_only_the_binary_parent() {
    let bytes = b"Open CASCADE Topology V4, (c) Open Cascade\nLocations invalid\nCurve2ds 0\n";
    decode(bytes, true, |ctx, output, payload| {
        // #224 comment6376511913 supersedes the former parent-level oracle:
        // the malformed marker itself is the failed sequential record.
        assert!(payload.unwrap().shape_set().unwrap().tshapes.is_empty());
        assert!(table_rows(output) == 0);
        assert_eq!(output.losses().len(), 1);
        assert!(output.losses()[0]
            .message
            .starts_with("freecad.brep_binary_record stream="));
        let parts = output.fidelity().parts().values().collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_binary"
                && matches!(part.state(), PartState::Decoded {})));
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_binary_header"
                && matches!(part.state(), PartState::Decoded {})));
        let failed = parts
            .iter()
            .find(|part| part.level().as_str() == "freecad.brep_binary_record")
            .unwrap();
        let marker_start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        assert_eq!(failed.offset(), u64_from_index(marker_start));
        assert_eq!(
            failed.byte_len(),
            u64_from_index(bytes.len() - marker_start)
        );
        assert!(matches!(failed.state(), PartState::GivenUp {}));
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 2);
        assert!(captures.iter().all(|capture| capture
            .reads()
            .values()
            .flatten()
            .all(|read| read.end <= u64_from_index(marker_start))));
    });
}

#[test]
fn failed_binary_record_keeps_the_accepted_prefix_and_never_probes_later_selectors_or_root() {
    let mut bytes =
        b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 3\n".to_vec();
    let first_start = bytes.len();
    bytes.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let failed_start = bytes.len();
    bytes.push(255);
    bytes.extend_from_slice(
        b"\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n",
    );
    bytes.extend_from_slice(
        &[-1_i32, -1, -1]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    decode(&bytes, true, |ctx, output, payload| {
        let payload = payload.unwrap();
        assert_eq!(payload.shape_set().unwrap().curves.len(), 1);
        assert_eq!(table_rows(output), 1);
        let native = records::payloads(ctx, output.native().namespace("fcstd").unwrap()).unwrap();
        assert_eq!(native[0].payload.shape_set().unwrap().curves.len(), 1);
        assert!(matches!(
            native[0].payload.shape_set().unwrap().curves[0],
            TextCurve::Line { .. }
        ));
        let parts = output.fidelity().parts().values().collect::<Vec<_>>();
        assert_eq!(parts.len(), 7);
        let binary_rows = parts
            .iter()
            .filter(|part| part.level().as_str() == "freecad.brep_binary_record")
            .collect::<Vec<_>>();
        assert_eq!(binary_rows.len(), 5);
        assert!(binary_rows
            .iter()
            .any(|part| part.offset() == u64_from_index(first_start)
                && part.byte_len() == 49
                && matches!(part.state(), PartState::Decoded {})));
        assert!(binary_rows
            .iter()
            .any(|part| part.offset() == u64_from_index(failed_start)
                && part.byte_len() == u64_from_index(bytes.len() - failed_start)
                && matches!(part.state(), PartState::GivenUp {})));
        assert!(!parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_binary_root"));
        assert_eq!(output.losses().len(), 1);
        let captures = ctx.take_record_captures().unwrap();
        let first = captures
            .iter()
            .find(|capture| capture.id() == OWNER)
            .unwrap();
        assert!(first.reads()["file"]
            .iter()
            .any(|range| range.start <= u64_from_index(first_start)
                && range.end >= u64_from_index(failed_start)));
        assert!(first.reads()["file"]
            .iter()
            .all(|range| range.end <= u64_from_index(failed_start)));
        assert_eq!(
            captures.len(),
            2,
            "only actual payload and census native records have scopes"
        );
        let census = captures
            .iter()
            .find(|capture| capture.id() == census_id(OWNER))
            .unwrap();
        // #214 §3.0: the independent census getter owns only its actual
        // family scalar, while the payload producer owns geometry getters.
        assert!(census.reads()["file"]
            .contains(&(u64_from_index(first_start)..u64_from_index(first_start + 1))));
        assert!(census.reads()["file"]
            .iter()
            .all(|range| range.end <= u64_from_index(first_start + 1)));
        assert!(captures
            .iter()
            .flat_map(|capture| capture.reads().values())
            .flatten()
            .all(|range| range.end <= u64_from_index(failed_start)));
    });
}

#[test]
fn text_surface_failure_preserves_prior_rows_and_independent_later_tables_even_with_non_utf8_damage(
) {
    for invalid in [b"255".as_slice(), b"\xff".as_slice()] {
        let mut bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 2\n1 0 0 0 0 0 1 1 0 0 0 1 0\n".to_vec();
        let failure_start = bytes.len();
        bytes.extend_from_slice(invalid);
        bytes
            .extend_from_slice(b"\nTriangulations 1\n0 0 0 0 0\nTShapes 1\nCo\n0101000\n*\n+1 0\n");
        let failure_end = bytes
            .windows(b"Triangulations 1".len())
            .position(|window| window == b"Triangulations 1")
            .unwrap();
        decode(&bytes, false, |ctx, output, payload| {
            let facts = payload.unwrap();
            let facts = facts.shape_set().unwrap();
            assert_eq!(facts.surfaces.len(), 1);
            assert_eq!(facts.triangulations.len(), 1);
            assert_eq!(facts.tshapes.len(), 1);
            assert_eq!(facts.roots.len(), 1);
            assert_eq!(table_rows(output), 3);
            let native =
                records::payloads(ctx, output.native().namespace("fcstd").unwrap()).unwrap();
            assert_eq!(native[0].payload.shape_set().unwrap().surfaces.len(), 1);
            assert_eq!(
                native[0].payload.shape_set().unwrap().triangulations.len(),
                1
            );
            let failures = output
                .fidelity()
                .parts()
                .values()
                .filter(|part| matches!(part.state(), PartState::GivenUp {}))
                .collect::<Vec<_>>();
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].level().as_str(), "freecad.brep_text_surface");
            assert_eq!(
                (failures[0].offset(), failures[0].byte_len()),
                (
                    u64_from_index(failure_start),
                    u64_from_index(failure_end - failure_start)
                )
            );
            assert_eq!(output.losses().len(), 1);
            let captures = ctx.take_record_captures().unwrap();
            assert!(captures
                .iter()
                .flat_map(|capture| capture.reads().get("file").into_iter().flatten())
                .all(|range| range.end <= u64_from_index(failure_start)
                    || range.start >= u64_from_index(failure_end)));
        });
    }
}

#[test]
fn terminated_text_shape_failure_keeps_later_source_indices_and_only_removes_missing_list_entries()
{
    let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3\nCo\n0101000\n*\nZZ\n0101000\n*\nCo\n0101000\n+3 0 +2 0 *\n+1 0\n";
    decode(bytes, false, |ctx, output, payload| {
        let payload = payload.unwrap();
        let facts = payload.shape_set().unwrap();
        assert_eq!(facts.tshapes.len(), 2);
        assert_eq!(facts.tshapes[1].children.len(), 1);
        assert_eq!(facts.tshapes[1].children[0].shape, 1);
        assert_eq!(facts.roots[0].shape, 2);
        assert_eq!(facts.indices[&7], [1, 3]);
        let native = records::payloads(ctx, output.native().namespace("fcstd").unwrap()).unwrap();
        assert_eq!(native[0].payload.shape_set().unwrap().indices[&7], [1, 3]);
        assert_eq!(output.losses().len(), 1);
        let failures = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].level().as_str(), "freecad.brep_text_tshape");
        assert_eq!(ctx.take_record_captures().unwrap().len(), 2);
    });
}

#[test]
fn unsupported_header_gives_up_the_header_without_forming_a_payload_record() {
    let bytes = b"CASCADE Topology V9, (c) Open Cascade\nLocations 0\n";
    decode(bytes, false, |ctx, output, payload| {
        assert!(payload.is_none());
        assert!(table_rows(output) == 0);
        assert_eq!(output.losses().len(), 1);
        let parts = output.fidelity().parts().values().collect::<Vec<_>>();
        assert_eq!(parts.len(), 2);
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_text_header"
                && matches!(part.state(), PartState::GivenUp {})));
        assert!(parts
            .iter()
            .any(|part| part.level().as_str() == "freecad.brep_text"
                && matches!(part.state(), PartState::Decoded {})));
        assert!(ctx.take_record_captures().unwrap().is_empty());
    });
}

#[test]
fn a_shape_representation_list_loses_only_the_entry_that_needs_a_missing_curve() {
    let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n255\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nEd 0 1 1 0 1 1 0 0 1 1 2 0 0 1 0\n0101000\n*\n+1 0\n";
    let removed = bytes
        .windows(b"1 2 0 0 1".len())
        .position(|window| window == b"1 2 0 0 1")
        .unwrap();
    decode(bytes, false, |ctx, output, payload| {
        let payload = payload.unwrap();
        let facts = payload.shape_set().unwrap();
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &facts.tshapes[0].geometry
        else {
            panic!("edge");
        };
        assert_eq!(representations.len(), 1);
        assert!(matches!(
            representations[0],
            TextEdgeRepresentation::Curve3d { curve: 1, .. }
        ));
        let native = records::payloads(ctx, output.native().namespace("fcstd").unwrap()).unwrap();
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &native[0].payload.shape_set().unwrap().tshapes[0].geometry
        else {
            panic!("edge")
        };
        assert_eq!(representations.len(), 1);
        assert_eq!(output.losses().len(), 1);
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == OWNER)
            .unwrap();
        assert!(capture.reads()["file"]
            .iter()
            .any(|range| range.start <= u64_from_index(removed + 2)
                && range.end >= u64_from_index(removed + 3)));
    });
}

#[test]
fn surviving_topology_keeps_its_original_shape_identity_after_a_middle_shape_is_given_up() {
    let source = |middle: &str| {
        format!("CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3\nVe 0 1 0 0 0 0 0101000 *\nVe 0 {middle} 0 0 0 0 0101000 *\nCo 0101000 +3 0 +2 0 *\n+1 0\n")
    };
    let transfer = |bytes: &[u8]| {
        decode(bytes, false, |ctx, output, payload| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.native = output.native().clone();
            let payload = ShapePayloadRecord {
                id: OWNER.to_owned(),
                property: "fcstd:native:property#fixture".to_owned(),
                entry: "fcstd:native:entry#fixture".to_owned(),
                rows: vec![],
                payload: payload.unwrap(),
            };
            crate::topology_transfer::transfer(ctx, &mut ir, &[payload], &[], &mut Vec::new())
                .unwrap();
            (
                ir.model
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect::<Vec<_>>(),
                ir.model
                    .vertices
                    .iter()
                    .map(|vertex| vertex.id.clone())
                    .collect::<Vec<_>>(),
            )
        })
    };
    let complete = transfer(source("2").as_bytes());
    let partial = transfer(source("NaN").as_bytes());
    assert_eq!(complete.0.len(), 1);
    assert_eq!(partial.0, complete.0);
    assert_eq!(complete.1.len(), 2);
    assert_eq!(partial.1.len(), 1);
    assert_eq!(partial.1[0], complete.1[0]);
}

#[test]
fn unreadable_table_count_leaves_later_shape_readable_and_prunes_its_missing_representation() {
    let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves invalid\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nEd 0 1 1 0 1 1 0 0 1 0\n0101000\n*\n+1 0\n";
    decode(bytes, false, |ctx, output, payload| {
        let payload = payload.unwrap();
        let facts = payload.shape_set().unwrap();
        assert!(facts.curves.is_empty());
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &facts.tshapes[0].geometry
        else {
            panic!("edge");
        };
        assert!(representations.is_empty());
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(output.losses().len(), 1);
        assert!(output.losses()[0]
            .message
            .starts_with("freecad.brep_text_table stream="));
        let native = records::payloads(ctx, output.native().namespace("fcstd").unwrap()).unwrap();
        assert_eq!(native[0].payload.shape_set().unwrap().tshapes.len(), 1);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native = output.native().clone();
        let payloads = records::payloads(ctx, ir.native.namespace("fcstd").unwrap()).unwrap();
        let mut losses = Vec::new();
        crate::topology_transfer::transfer(ctx, &mut ir, &payloads, &[], &mut losses).unwrap();
        assert!(ir.model.bodies.is_empty());
        assert_eq!(losses.len(), 1);
        assert!(losses[0]
            .message
            .contains(native[0].row_identity(7, 1).unwrap()));
    });
}

#[test]
fn native_envelope_has_one_table_home_and_materializes_current_sparse_rows() {
    let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n1 0 1 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nEd 0 1 1 0 1 1 0 0 1 1 2 0 0 1 0\n0101000\n*\n+1 0\n";
    decode(bytes, false, |ctx, output, _payload| {
        let mut namespace = output.native().namespace("fcstd").unwrap().clone();
        assert_eq!(
            namespace
                .arenas()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["carrier_census", "shape_payloads"]
        );
        let payload = namespace
            .arena_as::<ShapePayloadRecord>("shape_payloads")
            .unwrap()
            .remove(0);
        let mut wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire["text"]["curves"].as_array().unwrap().len(), 2);
        assert!(wire.get("rows").is_none());
        assert!(wire["text"].get("source_indices").is_some());
        wire["text"]["curves"][0]["origin"] = serde_json::json!([42.0, 0.0, 0.0]);
        let current: ShapePayloadRecord = serde_json::from_value(wire).unwrap();
        namespace
            .set_arena(ctx, "shape_payloads", &[current])
            .unwrap();
        let loaded = records::payloads(ctx, &namespace).unwrap();
        let TextCurve::Line { origin, .. } = &loaded[0].payload.shape_set().unwrap().curves[0]
        else {
            panic!("line")
        };
        assert_eq!(origin.get().x, 42.0);
        assert!(records::validate(ctx, &namespace, &loaded)
            .unwrap()
            .is_empty());
    });
}

#[test]
fn binary_header_unchecked_vendor_suffix_is_not_claimed_as_a_semantic_read() {
    let bytes = empty_binary();
    let suffix = b", (c) Open Cascade";
    let start = bytes
        .windows(suffix.len())
        .position(|window| window == suffix)
        .unwrap();
    decode(&bytes, true, |ctx, output, payload| {
        assert!(payload.is_some());
        assert!(output.losses().is_empty());
        let captures = ctx.take_record_captures().unwrap();
        let header_end = u64_from_index(b"Open CASCADE Topology V".len() + 1);
        for id in [OWNER.to_owned(), census_id(OWNER)] {
            let capture = captures.iter().find(|capture| capture.id() == id).unwrap();
            assert!(capture.reads()["file"]
                .iter()
                .any(|range| range.start == 0 && range.end >= header_end));
        }
        assert!(captures
            .iter()
            .flat_map(|capture| capture.reads().values())
            .flatten()
            .all(|range| range.end <= u64_from_index(start)
                || range.start >= u64_from_index(start + suffix.len())));
    });
}

#[test]
fn binary_root_has_exact_located_extent_or_a_bounded_cut_short_attempt() {
    for length in [0, 1, 11, 12, 13, 24] {
        let mut bytes = empty_binary();
        bytes.extend(std::iter::repeat_n(255, length));
        decode(&bytes, true, |ctx, output, payload| {
            let roots = output
                .fidelity()
                .parts()
                .values()
                .filter(|part| part.level().as_str() == "freecad.brep_binary_root")
                .collect::<Vec<_>>();
            if length > 12 {
                assert!(payload.is_none(), "length {length}");
                assert!(roots.is_empty(), "length {length}");
                let parent = output
                    .fidelity()
                    .parts()
                    .values()
                    .find(|part| part.level().as_str() == "freecad.brep_binary")
                    .unwrap();
                assert!(matches!(parent.state(), PartState::GivenUp {}));
                assert!(ctx.take_record_captures().unwrap().is_empty());
            } else {
                assert!(payload.is_some(), "length {length}");
                assert_eq!(roots.len(), usize::from(length != 0));
                if let Some(root) = roots.first() {
                    assert_eq!(root.byte_len(), u64_from_index(length));
                    assert_eq!(root.offset(), u64_from_index(bytes.len() - length));
                    assert_eq!(matches!(root.state(), PartState::Decoded {}), length == 12);
                }
            }
        });
    }
}

#[test]
fn malformed_second_binary_marker_retains_first_table_records_and_stops_the_schedule() {
    let mut bytes = b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 1\n".to_vec();
    let location_start = bytes.len();
    bytes.push(1);
    for value in [
        1.0_f64, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 6.0, 0.0, 0.0, 1.0, 7.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(b'\n');
    let failed_start = bytes.len();
    bytes.extend_from_slice(b"Curve2ds invalid\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n");
    decode(&bytes, true, |ctx, output, payload| {
        assert_eq!(payload.unwrap().shape_set().unwrap().locations.len(), 1);
        assert_eq!(table_rows(output), 1);
        let failures = output
            .fidelity()
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .collect::<Vec<_>>();
        let [failed] = failures.as_slice() else {
            panic!("{failures:?}")
        };
        assert_eq!(failed.level().as_str(), "freecad.brep_binary_record");
        assert_eq!(failed.offset(), u64_from_index(failed_start));
        assert_eq!(
            failed.byte_len(),
            u64_from_index(bytes.len() - failed_start)
        );
        assert_eq!(output.losses().len(), 1);
        let captures = ctx.take_record_captures().unwrap();
        let owner = captures
            .iter()
            .find(|capture| capture.id() == OWNER)
            .unwrap();
        assert!(owner.reads()["file"]
            .iter()
            .any(|read| read.start <= u64_from_index(location_start)
                && read.end >= u64_from_index(failed_start - 1)));
        assert!(captures
            .iter()
            .flat_map(|capture| capture.reads().values().flatten())
            .all(|read| read.end <= u64_from_index(failed_start)));
        assert_eq!(
            output
                .fidelity()
                .parts()
                .values()
                .filter(|part| part.level().as_str() == "freecad.brep_binary_record")
                .count(),
            3
        );
    });
}

#[test]
fn unread_tables_and_roots_remain_absent_while_readable_empty_tables_stay_present() {
    let binary =
        b"Open CASCADE Topology V4, (c) Open Cascade\nLocations 0\nCurve2ds invalid\nCurves 0\n";
    decode(binary, true, |ctx, output, _| {
        let namespace = output.native().namespace("fcstd").unwrap();
        let payload = records::payloads(ctx, namespace).unwrap().remove(0);
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire["binary"]["locations"], serde_json::json!([]));
        for field in [
            "curve2ds",
            "curves",
            "polygons3d",
            "polygons_on_triangulations",
            "surfaces",
            "triangulations",
            "tshapes",
            "roots",
        ] {
            assert!(
                wire["binary"].get(field).is_none(),
                "unread {field} is absent"
            );
        }
        let loaded: ShapePayloadRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(loaded).unwrap(), wire);
    });
    let text=b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves invalid\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*\n";
    decode(text, false, |ctx, output, _| {
        let payload = records::payloads(ctx, output.native().namespace("fcstd").unwrap())
            .unwrap()
            .remove(0);
        let wire = serde_json::to_value(&payload).unwrap();
        assert!(wire["text"].get("curves").is_none());
        assert!(wire["text"]["section_counts"].get("Curves").is_none());
        assert_eq!(wire["text"]["surfaces"], serde_json::json!([]));
        assert_eq!(wire["text"]["tshapes"], serde_json::json!([]));
        assert_eq!(wire["text"]["roots"], serde_json::json!([]));
        let loaded: ShapePayloadRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(loaded).unwrap(), wire);
    });
    decode(&empty_binary(), true, |ctx, output, _| {
        let payload = records::payloads(ctx, output.native().namespace("fcstd").unwrap())
            .unwrap()
            .remove(0);
        let wire = serde_json::to_value(payload).unwrap();
        let expected = serde_json::json!({"topology_version":4,"locations":[],"curve2ds":[],"curves":[],"polygons3d":[],"polygons_on_triangulations":[],"surfaces":[],"triangulations":[],"tshapes":[],"roots":[]});
        assert_eq!(
            wire["binary"], expected,
            "healthy base table fields stay exact"
        );
    });
}
