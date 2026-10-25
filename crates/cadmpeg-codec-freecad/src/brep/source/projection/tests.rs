// SPDX-License-Identifier: Apache-2.0
//! Original placement inputs feed actual neutral constructors and failure closure.
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

fn source(matrix_x: &str) -> String {
    format!("CASCADE Topology V3, (c) Open Cascade\nLocations 2\n1 1 0 0 {matrix_x} 0 1 0 0 0 0 1 0\n2 1 2 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 2\nVe 0 1 0 0 0 0 0101000 *\nCo 0101000 +2 2 *\n+1 1\n")
}

#[test]
fn actual_root_and_factor_location_getters_feed_points_and_body_and_license_missing_root_context() {
    let mut previous = None;
    for matrix_x in ["3", "NaN"] {
        let bytes = source(matrix_x);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes.as_bytes(), &arena, &policy).unwrap();
        let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
        let mut marks = RowMarks::default();
        let owner = "fcstd:native:shape-payload#placement";
        let payload = ctx
            .decode_part(
                &mut output,
                root,
                spec("freecad.brep_text", Vec::new()).unwrap(),
                |stage| {
                    let Some((facts, version)) =
                        text_shape_marked(&ctx, root, owner, stage, &mut marks)?
                    else {
                        return Ok(None);
                    };
                    let record = records::with_rows(
                        &ctx,
                        stage.native(),
                        ShapePayloadRecord {
                            id: owner.to_owned(),
                            property: "fcstd:native:property#Shape".to_owned(),
                            entry: "fcstd:native:entry#Shape.brp".to_owned(),
                            rows: Vec::new(),
                            payload: ShapePayload::Text { facts, version },
                        },
                    )?;
                    stage.insert_native(&ctx, "shape_payloads", record.clone())?;
                    Ok(Some(record))
                },
            )
            .unwrap()
            .flatten()
            .unwrap();
        let mut sources = ProjectionSources::default();
        sources.insert(&payload, root, false, marks);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native = output.native().clone();
        crate::topology_transfer::transfer_with_sources(
            &ctx,
            &mut ir,
            &[payload],
            &[],
            &mut Vec::new(),
            Some(&sources),
        )
        .unwrap();
        let captures = ctx.take_record_captures().unwrap();
        if matrix_x == "3" {
            assert_eq!(ir.model.bodies.len(), 1);
            assert_eq!(ir.model.points.len(), 1);
            assert_eq!(ir.model.points[0].position().get().x, 7.0);
            assert_eq!(ir.model.bodies[0].transform.unwrap().rows()[0][3], 3.0);
            let point = captures
                .iter()
                .find(|capture| capture.id() == ir.model.points[0].id.as_str())
                .unwrap();
            let body = captures
                .iter()
                .find(|capture| capture.id() == ir.model.bodies[0].id.as_str())
                .unwrap();
            let literal = bytes.find("1 1 0 0 3").unwrap() + "1 1 0 0 ".len();
            let factor = bytes.find("2 1 2 0").unwrap();
            for capture in [point, body] {
                assert!(capture.reads()["file"]
                    .iter()
                    .any(|read| read.start <= u64_from_index(literal)
                        && u64_from_index(literal + 1) <= read.end));
            }
            assert!(point.reads()["file"]
                .iter()
                .any(|read| read.start <= u64_from_index(factor + 2)
                    && u64_from_index(factor + 3) <= read.end));
            previous = Some(point.reads()["file"].clone());
        } else {
            assert!(ir.model.bodies.is_empty());
            assert!(ir.model.points.is_empty());
            assert!(ir
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<ShapePayloadRecord>("shape_payloads")
                .unwrap()
                .iter()
                .any(|payload| payload.id == owner));
            let failed = output
                .fidelity()
                .parts()
                .values()
                .find(|part| {
                    part.level().as_str() == "freecad.brep_text_location"
                        && matches!(part.state(), PartState::GivenUp {})
                })
                .unwrap();
            assert!(previous
                .as_ref()
                .unwrap()
                .iter()
                .any(|read| read.start < failed.offset() + failed.byte_len()
                    && failed.offset() < read.end));
            assert!(!captures
                .iter()
                .any(|capture| capture.id().starts_with("fcstd:model:point#")));
        }
    }
}

#[test]
fn placed_procedural_replica_owns_the_actual_surface_and_location_inputs() {
    let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 1\n1 1 0 0 3 0 1 0 0 0 0 1 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n10 0 1 0 1 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 1\nFa 0 0.001 1 1 0101000 *\n+1 0\n";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let owner = "fcstd:native:shape-payload#placed-trim";
    let mut marks = RowMarks::default();
    let payload = ctx
        .decode_part(
            &mut output,
            root,
            spec("freecad.brep_text", Vec::new()).unwrap(),
            |stage| {
                let (facts, version) = text_shape_marked(&ctx, root, owner, stage, &mut marks)?
                    .ok_or_else(|| CodecError::malformed("source header did not decode"))?;
                let payload = records::with_rows(
                    &ctx,
                    stage.native(),
                    ShapePayloadRecord {
                        id: owner.to_owned(),
                        property: "fcstd:native:property#Shape".to_owned(),
                        entry: "fcstd:native:entry#Shape.brp".to_owned(),
                        rows: Vec::new(),
                        payload: ShapePayload::Text { facts, version },
                    },
                )?;
                stage.insert_native(&ctx, "shape_payloads", payload.clone())?;
                Ok(payload)
            },
        )
        .unwrap()
        .unwrap();
    assert!(output.losses().is_empty());
    let mut sources = ProjectionSources::default();
    sources.insert(&payload, root, false, marks);
    let mut curves = crate::brep::CurveTransfer::default();
    let surfaces = crate::brep::transfer_text_surfaces(
        &ctx,
        std::slice::from_ref(&payload),
        &[],
        &mut curves,
        Some(&sources),
    )
    .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native = output.native().clone();
    ir.model.curves = curves.curves;
    ir.model
        .add_procedural_curves(curves.procedural)
        .unwrap()
        .unwrap();
    ir.model.surfaces = surfaces.surfaces;
    ir.model
        .add_procedural_surfaces(surfaces.procedural)
        .unwrap()
        .unwrap();
    crate::topology_transfer::transfer_with_sources(
        &ctx,
        &mut ir,
        &[payload],
        &[],
        &mut Vec::new(),
        Some(&sources),
    )
    .unwrap();
    let replicas = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(replicas.len(), 1);
    let replica = replicas[0];
    assert_eq!(replica.native_ref.as_deref(), Some(owner));
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { transform, .. } =
        replica.definition()
    else {
        panic!("replica");
    };
    assert_eq!(transform.rows()[0][3], 3.0);
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == replica.id.as_str())
        .unwrap();
    let surface = bytes
        .windows(b"10 0 1 0 1".len())
        .position(|window| window == b"10 0 1 0 1")
        .unwrap();
    let location = bytes
        .windows(b"1 1 0 0 3".len())
        .position(|window| window == b"1 1 0 0 3")
        .unwrap()
        + b"1 1 0 0 ".len();
    for offset in [surface, location] {
        assert!(capture.reads()["file"]
            .iter()
            .any(|read| read.start <= u64_from_index(offset)
                && u64_from_index(offset + 1) <= read.end));
    }
}
