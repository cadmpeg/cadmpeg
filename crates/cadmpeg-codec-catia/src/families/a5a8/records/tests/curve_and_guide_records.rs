// SPDX-License-Identifier: Apache-2.0
//! curve and guide records tests.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_ir::codec::Codec;

use super::{
    a5_freeform_curve_stream, a5_guide_curve_stream, a5_guide_curve_stream_with_count,
    a6_freeform_curve_stream, a8_catpart, a8_elided_surface_stream,
    a8_elided_surface_stream_with_native_vertex_chain, a8_freeform_curve_stream,
    a8_freeform_curve_stream_with_count, a8_pcurve_stream, inner_no_directory_a8_catpart, le_f64,
    object_main_catpart, parsed_a5_freeform_curves, parsed_a8_freeform_curves, CatiaCodec, Cursor,
    DecodeOptions, EditableDecodeResult, Point3, SolvedSurfaceGeometry, SurfaceGeometry, Variant,
};

#[test]
fn rolling_ball_parsers_accept_finite_nonzero_radii() {
    for radius in [1e-200, 1e200, 1e308] {
        let mut a5 = a5_freeform_curve_stream();
        a5[28..36].copy_from_slice(&le_f64(radius));
        a5[60..68].copy_from_slice(&le_f64(radius));
        let [curve] = parsed_a5_freeform_curves(&a5)
            .try_into()
            .expect("one consolidated rolling-ball jet");
        assert_eq!(curve.sites[0].site.radius(), radius);

        let mut a8 = a8_freeform_curve_stream();
        a8[36..44].copy_from_slice(&le_f64(radius));
        a8[68..76].copy_from_slice(&le_f64(radius));
        let [curve] = parsed_a8_freeform_curves(&a8)
            .try_into()
            .expect("one common-form rolling-ball jet");
        assert_eq!(curve.sites[0].site.radius(), radius);
    }
}

#[test]
fn rolling_ball_parsers_reject_scale_relative_radius_disagreement() {
    let tiny = 1e-200;
    let mut bytes = a5_freeform_curve_stream();
    bytes[28..36].copy_from_slice(&le_f64(tiny));
    bytes[60..68].copy_from_slice(&le_f64(2.0 * tiny));
    bytes[100..108].copy_from_slice(&le_f64(std::f64::consts::PI));
    assert!(parsed_a5_freeform_curves(&bytes).is_empty());
}

#[test]
fn consolidated_curve_parser_reads_width2_frame() {
    let curves = parsed_a5_freeform_curves(&a6_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curves[0].knots(ctx))
            .expect("service resource budget")
            .len(),
        2
    );
    assert_eq!(curves[0].sites[1].site.radius(), 2.0);
}

#[test]
fn guide_curve_parser_reads_position_and_unit_direction_jet() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input limit");
    let curves = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &a5_guide_curve_stream())
            .expect("service decode")
    });
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].sites[0].point.get(), [0.0, 0.0, 0.0]);
    assert_eq!(curves[0].sites.len(), 2);
    let points = curves[0]
        .sites
        .iter()
        .map(|site| site.point.get())
        .collect::<Vec<_>>();
    let derivatives = vec![[0.0; 3]; 2];
    let (knots, controls) = crate::nurbs::quintic_jet_bspline(
        &ctx,
        curves[0].degree,
        &curves[0].knots(&ctx).expect("service resource budget"),
        &points,
        &derivatives,
        &derivatives,
    )
    .expect("service resource budget")
    .expect("exact 3D quintic jet");
    assert_eq!(knots, [vec![0.0; 6], vec![1.0; 6]].concat());
    assert_eq!(
        controls.first().map(|point| point.get()),
        Some([0.0, 0.0, 0.0])
    );
    assert_eq!(
        controls.last().map(|point| point.get()),
        Some([2.0, 3.0, 4.0])
    );
}

#[test]
fn guide_curve_parser_refuses_nonunit_site_direction() {
    let mut bytes = a5_guide_curve_stream();
    bytes[52..60].copy_from_slice(&le_f64(2.0));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &bytes).expect("service decode")
    })
    .is_empty());
}

#[test]
fn a5_guide_sites_refuse_collection_limit_before_materialization() {
    let bytes = a5_guide_curve_stream();
    let limited = crate::test_support::with_collection_limit(1, |ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &bytes)
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_a5_guide_sites"));
    let curves = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &bytes)
    })
    .expect("service collection budget");
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].sites.len(), 2);
}

#[test]
fn guide_curve_parser_accepts_frame_bounded_site_count() {
    let curves = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(
            ctx,
            &a5_guide_curve_stream_with_count(4097),
        )
        .expect("service decode")
    });
    assert_eq!(curves.len(), 1);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curves[0].knots(ctx))
            .expect("service resource budget")
            .len(),
        4097
    );
    assert_eq!(curves[0].sites.len(), 4097);
}

#[test]
fn guide_curve_parser_rejects_nonfinite_jet_channels() {
    for offset in [12, 124, 220] {
        let mut bytes = a5_guide_curve_stream();
        bytes[offset..offset + 8].copy_from_slice(&le_f64(f64::NAN));
        assert!(
            crate::test_support::with_service_context(|ctx| {
                crate::families::a5a8::records::a5_guide_curves(ctx, &bytes)
                    .expect("service decode")
            })
            .is_empty(),
            "offset {offset}"
        );
    }

    let mut repeated_knot = a5_guide_curve_stream();
    repeated_knot[20..28].copy_from_slice(&le_f64(0.0));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &repeated_knot)
            .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_curve_parser_reads_common_form_rolling_ball_jet() {
    let curves = parsed_a8_freeform_curves(&a8_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].object_id, 0x1234_5678);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curves[0]
            .multiplicities(ctx)
            .expect("service decode")),
        vec![6, 6]
    );
    assert_eq!(curves[0].sites[1].site.radius(), 2.0);

    let mut repeated_knot = a8_freeform_curve_stream();
    repeated_knot[26..34].copy_from_slice(&le_f64(0.0));
    assert!(parsed_a8_freeform_curves(&repeated_knot).is_empty());

    let mut invalid_endpoint_multiplicity = a8_freeform_curve_stream();
    invalid_endpoint_multiplicity[34] = 21;
    assert!(parsed_a8_freeform_curves(&invalid_endpoint_multiplicity).is_empty());
}

#[test]
fn a8_freeform_sites_refuse_collection_limit_before_materialization() {
    assert_a8_freeform_collection_refusal(1, "catia_a8_freeform_sites");
}

#[test]
fn a8_freeform_curves_refuse_collection_limit_before_retention() {
    assert_a8_freeform_collection_refusal(2, "catia_a8_freeform_curves");
}

fn assert_a8_freeform_collection_refusal(limit: u64, operation: &'static str) {
    let bytes = a8_freeform_curve_stream();
    let result = crate::test_support::with_collection_limit(limit, |ctx| {
        crate::families::a5a8::records::a8_freeform_curves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == operation
    ));
    assert_eq!(parsed_a8_freeform_curves(&bytes).len(), 1);
}

#[test]
fn a8_jet_stations_refuse_collection_limit_before_materialization() {
    let jet = parsed_a8_freeform_curves(&a8_freeform_curve_stream()).remove(0);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::families::a5a8::records::rolling_ball_jet_definition(ctx, &jet)
    };
    assert!(matches!(crate::test_support::with_collection_limit(1, run),
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_a8_jet_stations"
    ));
    assert!(crate::test_support::with_service_context(run)
        .expect("service decode")
        .is_some());
}

#[test]
fn a8_jet_multiplicities_refuse_collection_limit_before_materialization() {
    let jet = parsed_a8_freeform_curves(&a8_freeform_curve_stream()).remove(0);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| jet.multiplicities(ctx);
    assert!(matches!(crate::test_support::with_collection_limit(1, run),
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_a8_jet_multiplicities"
    ));
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service decode"),
        vec![6, 6]
    );
}

#[test]
fn a8_curve_parser_accepts_frame_bounded_site_count() {
    let curves = parsed_a8_freeform_curves(&a8_freeform_curve_stream_with_count(8193));
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].sites.len(), 8193);
}

#[test]
fn a8_curve_parser_accepts_each_object_frame_flag() {
    for flag in [0x03, 0x13, 0x83] {
        let mut bytes = a8_freeform_curve_stream();
        bytes[1] = flag;
        assert_eq!(
            parsed_a8_freeform_curves(&bytes).len(),
            1,
            "flag {flag:#04x}"
        );
    }

    let mut malformed = a8_freeform_curve_stream();
    malformed[1] = 0x23;
    assert!(parsed_a8_freeform_curves(&malformed).is_empty());
}

#[test]
fn indexed_a5_record_decoders_match_one_shot_wrappers() {
    let freeform = a5_freeform_curve_stream();
    let records = crate::wire::records::consolidated_records(&freeform);
    let one_shot = parsed_a5_freeform_curves(&freeform);
    let indexed = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_freeform_curves_from_records(ctx, &freeform, &records)
            .expect("service decode")
    });
    assert_eq!(one_shot.len(), indexed.len());
    for (one_shot, indexed) in one_shot.iter().zip(&indexed) {
        assert_eq!(one_shot.pos, indexed.pos);
        assert_eq!(one_shot.header_token, indexed.header_token);
        assert_eq!(one_shot.sites, indexed.sites);
    }

    let guide = a5_guide_curve_stream();
    let records = crate::wire::records::consolidated_records(&guide);
    let one_shot = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves(ctx, &guide).expect("service decode")
    });
    let indexed = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_guide_curves_from_records(ctx, &guide, &records)
            .expect("service decode")
    });
    assert_eq!(one_shot.len(), indexed.len());
    for (one_shot, indexed) in one_shot.iter().zip(&indexed) {
        assert_eq!(one_shot.pos, indexed.pos);
        assert_eq!(one_shot.header_token, indexed.header_token);
        assert_eq!(one_shot.degree, indexed.degree);
        assert_eq!(one_shot.sites, indexed.sites);
    }

    let nurbs = a5_nurbs_curve_stream();
    let records = crate::wire::records::consolidated_records(&nurbs);
    assert_eq!(
        parsed_a5_nurbs_curves(&nurbs),
        crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a5_nurbs_curves_from_records(
                ctx,
                &nurbs,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
    );
}

fn parsed_a5_nurbs_curves(data: &[u8]) -> Vec<crate::families::a5a8::records::A5NurbsCurve> {
    crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_nurbs_curves(ctx, data).expect("service decode")
    })
}

fn a5_nurbs_curve_stream() -> Vec<u8> {
    let knots = [-2.220_264_955_47_f64, 0.0, 2.220_264_955_47];
    let points = [
        [25.024_609_677_8, 20.779_735_044_5, 13.0],
        [24.316_927_644_1, 21.223_788_035_6, 13.0],
        [23.708_153_935, 21.667_841_026_7, 13.0],
        [23.236_619_670_7, 22.111_894_017_8, 13.0],
        [22.763_380_329_3, 23.0, 13.0],
        [23.236_619_670_7, 23.888_105_982_2, 13.0],
        [23.708_153_935, 24.332_158_973_3, 13.0],
        [24.316_927_644_1, 24.776_211_964_4, 13.0],
        [25.024_609_677_8, 25.220_264_955_5, 13.0],
    ];
    let mut payload = vec![0x15, 0x0d, 0x0c];
    for knot in knots {
        payload.extend_from_slice(&knot.to_le_bytes());
    }
    payload.push(0x01);
    for point in points {
        for coordinate in point {
            payload.extend_from_slice(&f64::to_le_bytes(coordinate));
        }
    }
    payload.extend_from_slice(&[0x05, 0x09]);
    for value in [0.0, knots[2], 1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    assert_eq!(payload.len(), 280);
    let mut record = vec![0xa5, 0x13, 0x16];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x0d);
    record.extend(payload);
    record
}

fn a5_nurbs_curve_stream_with_knot_count(knot_count: usize) -> Vec<u8> {
    assert_eq!(knot_count, 8193);
    let mut payload = vec![0x15, 0x08, 0x01, 0x20, 0x0c];
    for knot in 0..knot_count {
        payload.extend_from_slice(&f64::from(u32::try_from(knot).unwrap()).to_le_bytes());
    }
    payload.push(0x01);
    for _ in 0..(3 * knot_count) {
        for _ in 0..3 {
            payload.extend_from_slice(&0.0f64.to_le_bytes());
        }
    }
    payload.extend_from_slice(&[0x05, 0x09]);
    for value in [
        0.0,
        f64::from(u32::try_from(knot_count - 1).unwrap()),
        1.0,
        0.0,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    let mut record = vec![0xa5, 0x13, 0x16];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x0d);
    record.extend(payload);
    record
}

#[test]
fn a5_nurbs_curve_parser_expands_the_degree_five_knot_multiplicities() {
    let curves = parsed_a5_nurbs_curves(&a5_nurbs_curve_stream());
    let [curve] = curves.as_slice() else {
        panic!("one degree-five curve");
    };
    assert_eq!(curve.geometry.degree(), 5);
    assert_eq!(curve.geometry.control_points().len(), 9);
    assert_eq!(curve.geometry.knots().len(), 15);
    assert_eq!(curve.geometry.knots()[..6], [-2.220_264_955_47; 6]);
    assert_eq!(curve.geometry.knots()[6..9], [0.0; 3]);
    assert_eq!(curve.geometry.knots()[9..], [2.220_264_955_47; 6]);
    assert!(curve.geometry.weights().is_none());
}

#[test]
fn a5_nurbs_curve_parser_accepts_frame_bounded_knot_count() {
    let curves = parsed_a5_nurbs_curves(&a5_nurbs_curve_stream_with_knot_count(8193));
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].geometry.control_points().len(), 24_579);
    assert_eq!(curves[0].geometry.knots().len(), 24_585);
}

#[test]
fn a5_nurbs_distinct_knots_refuse_collection_limit_before_materialization() {
    assert_a5_nurbs_collection_refusal(2, "catia_a5_nurbs_distinct_knots");
}

#[test]
fn a5_nurbs_control_points_refuse_collection_limit_before_materialization() {
    assert_a5_nurbs_collection_refusal(11, "catia_a5_nurbs_control_points");
}

#[test]
fn a5_nurbs_expanded_knots_refuse_collection_limit_before_materialization() {
    assert_a5_nurbs_collection_refusal(26, "catia_a5_nurbs_expanded_knots");
}

#[test]
fn a5_nurbs_curve_collection_refuses_before_retention() {
    assert_a5_nurbs_collection_refusal(27, "catia_a5_nurbs_curves");
}

fn assert_a5_nurbs_collection_refusal(limit: u64, operation: &'static str) {
    let bytes = a5_nurbs_curve_stream();
    let result = crate::test_support::with_collection_limit(limit, |ctx| {
        crate::families::a5a8::records::a5_nurbs_curves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == operation
    ));
    assert_eq!(parsed_a5_nurbs_curves(&bytes).len(), 1);
}

#[test]
fn a5_nurbs_curve_parser_rejects_nonfinite_knots_and_control_points() {
    let mut nonfinite_knot = a5_nurbs_curve_stream();
    nonfinite_knot[11..19].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parsed_a5_nurbs_curves(&nonfinite_knot).is_empty());

    let mut nonfinite_control_point = a5_nurbs_curve_stream();
    nonfinite_control_point[36..44].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parsed_a5_nurbs_curves(&nonfinite_control_point).is_empty());
}

#[test]
fn a5_nurbs_curve_parser_rejects_broken_frame_invariants() {
    let valid = a5_nurbs_curve_stream();
    for offset in [8, 9, 10, 27, 35, 252, 253, 254, 262, 270, 278, 286, 287] {
        let mut broken = valid.clone();
        broken[offset] ^= 1;
        assert!(
            parsed_a5_nurbs_curves(&broken).is_empty(),
            "offset {offset}"
        );
    }
}

#[test]
fn decode_geometry_fallback_transfers_an_external_a8_pole_grid() {
    let file = object_main_catpart(&a8_elided_surface_stream());
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.poles().len(), 9);
    assert_eq!(
        surface.poles().into_iter().nth(8).unwrap(),
        Point3::new(8.0, 2.0, 2.0)
    );
}

#[test]
fn decode_float_packed_stream_transfers_an_elided_a8_surface_with_native_topology() {
    let stream = a8_elided_surface_stream_with_native_vertex_chain();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &stream, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("generated A8 topology");
    assert!(graph.complete);
    assert_eq!(graph.faces.len(), 1);
    assert_eq!(graph.loops.len(), 1);
    assert_eq!(graph.pcurves.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(
        graph
            .vertices
            .logical_vertices()
            .iter()
            .map(|vertex| vertex.object_id)
            .collect::<Vec<_>>(),
        [600, 601, 602]
    );
    assert_eq!(
        graph
            .vertices
            .logical_vertices()
            .iter()
            .map(|vertex| crate::test_support::test_b5::coordinates(vertex.point))
            .collect::<Vec<_>>(),
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode elided A8 surface topology");
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("NURBS surface");
    };
    assert_eq!(
        surface.poles().into_iter().nth(8).unwrap(),
        Point3::new(1.0, 1.0, 0.0)
    );
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::loss::LossCategory::Geometry
                | cadmpeg_ir::report::loss::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_object_stream_does_not_promote_unbound_a8_pcurve() {
    let file = object_main_catpart(&a8_pcurve_stream());
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode unbound object-stream pcurve");
    assert!(decoded.ir().model.pcurves.is_empty());
    assert!(!decoded.ir().native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn decode_object_stream_transfers_a8_rolling_ball_jet() {
    let file = object_main_catpart(&a8_freeform_curve_stream());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(
            ctx,
            file.clone()
        ))
        .expect("service resource budget")
        .variant,
        Variant::FloatPackedInnerNoFbb
    );
    let decoded = EditableDecodeResult::from(
        CatiaCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("decode rolling-ball object stream"),
    );
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet(jet) =
        procedural.definition()
    else {
        panic!("rolling-ball jet");
    };
    let degree = jet.degree();
    let stations = jet.stations();
    let knots: Vec<_> = stations.iter().map(|station| station.knot.get()).collect();
    let multiplicities: Vec<_> = stations
        .iter()
        .map(|station| station.multiplicity)
        .collect();
    let sites: Vec<_> = stations.iter().map(|station| &station.site).collect();
    assert_eq!(degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(sites[1].angle.get(), std::f64::consts::FRAC_PI_2);
    let provenance = &decoded.source_fidelity().annotations.provenance[procedural.id.as_str()];
    assert_eq!(provenance.stream(), "catia:object_stream_a8_03_32");
    let tag = provenance
        .tag
        .as_deref()
        .expect("rolling-ball provenance tag");
    assert!(tag.contains("object_id:12345678"));
    assert!(tag.contains("multiplicities:[6, 6]"));
    assert_eq!(
        decoded.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:12345678"))
    );
}

#[test]
fn decode_float_packed_stream_transfers_a8_nurbs() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(
            ctx,
            a8_catpart()
        ))
        .expect("service resource budget")
        .variant,
        Variant::FloatPackedInnerNoFbb
    );
    let mut cur = Cursor::new(a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

#[test]
fn decode_inner_no_directory_transfers_a8_nurbs() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(
            ctx,
            inner_no_directory_a8_catpart()
        ))
        .expect("service resource budget")
        .variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}
