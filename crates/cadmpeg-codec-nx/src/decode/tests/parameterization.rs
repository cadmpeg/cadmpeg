// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, PcurveGeometry, PcurveNurbs,
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::decode::SerializedSupportUv;
use crate::test_support::*;
use crate::NxCodec;

#[test]
fn offset_surface_parameter_solver_preserves_support_parameters() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir()
        .model
        .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
        .expect("offset surface owner")
        .clone();
    let expected = Point2::new(12.0, 7.0);
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(result.ir()),
        &surface,
        expected.u,
        expected.v,
    )
    .unwrap();

    let actual =
        crate::decode::offset_surface_parameters(result.ir(), &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let mut translated = result.ir().clone();
    for carrier in &mut translated.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut carrier.geometry {
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
        }
    }
    let translated_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let translated_parameters = crate::decode::offset_surface_parameters_with_tolerance(
        &translated,
        &surface,
        translated_point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.1)),
        Some(1.0e-3),
    )
    .expect("exact offset tangents are independent of model-space magnitude");
    assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

    let nested_surface =
        cadmpeg_ir::ids::SurfaceId::mint("synthetic:nested-offset").expect("identity grammar");
    let nested_construction =
        cadmpeg_ir::ids::ProceduralSurfaceId::mint("synthetic:nested-offset-construction")
            .expect("identity grammar");
    translated
        .model
        .surfaces
        .push(cadmpeg_ir::geometry::Surface {
            id: nested_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: nested_construction.clone(),
                cache: None,
            },
            source_object: None,
        });
    translated
        .model
        .procedural_surfaces
        .push(cadmpeg_ir::geometry::ProceduralSurface::new(
            nested_construction,
            ProceduralSurfaceDefinition::Offset {
                support: surface,
                distance: -0.75,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                ),
            },
            None,
        ));
    let nested_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &nested_surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let nested_parameters = crate::decode::offset_surface_parameters_with_tolerance(
        &translated,
        &nested_surface,
        nested_point,
        Some(Point2::new(expected.u - 0.1, expected.v + 0.1)),
        Some(1.0e-3),
    )
    .expect("nested offsets share the exact base-surface normal derivative");
    assert!((nested_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((nested_parameters.v - expected.v).abs() < 1.0e-3);
}

#[test]
fn offset_surface_parameter_solver_accepts_a_seed_within_fit_tolerance() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir()
        .model
        .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
        .expect("offset surface owner")
        .clone();
    let seed = Point2::new(12.0, 7.0);
    let mut point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(result.ir()),
        &surface,
        seed.u,
        seed.v,
    )
    .unwrap();
    point.x += 0.01;

    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        result.ir(),
        &surface,
        point,
        Some(seed),
        Some(0.02),
    )
    .unwrap();

    assert_eq!(actual, seed);

    let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(256);
    let local = crate::decode::offset::refine_offset_surface_parameters_with_index_and_budget(
        &index,
        &surface,
        point,
        seed,
        0.02,
        &geometry_budget,
    )
    .expect("a local fit inside the relation tolerance is admissible");
    assert!((local.u - seed.u).abs() <= 0.02);
    assert!((local.v - seed.v).abs() <= 0.02);
}

#[test]
fn offset_surface_parameter_solver_retries_a_bad_continuation_seed() {
    use cadmpeg_ir::geometry::{NurbsSurface, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    const FIT_TOLERANCE: f64 = 0.000_001;
    const PARAMETER_TOLERANCE: f64 = 0.001;

    let support = SurfaceId::mint("synthetic:wavy-support").expect("identity grammar");
    let offset = SurfaceId::mint("synthetic:wavy-offset").expect("identity grammar");
    let construction =
        ProceduralSurfaceId::mint("synthetic:wavy-offset-construction").expect("identity grammar");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                3,
                1,
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                4,
                2,
                vec![
                    Point3::new(-3.0, 0.0, 0.0),
                    Point3::new(-3.0, 0.0, 1.0),
                    Point3::new(3.0, 2.0, 0.0),
                    Point3::new(3.0, 2.0, 1.0),
                    Point3::new(-3.0, 4.0, 0.0),
                    Point3::new(-3.0, 4.0, 1.0),
                    Point3::new(3.0, 6.0, 0.0),
                    Point3::new(3.0, 6.0, 1.0),
                ],
                None,
                false,
                false,
                false,
            )
            .expect("valid wavy support"),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: offset.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Offset {
            support,
            distance: 0.75,
            u_sense: None,
            v_sense: None,
            support_extension: None,
            extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
            ),
        },
        None,
    ));

    let expected = Point2::new(0.2, 0.45);
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&ir),
        &offset,
        expected.u,
        expected.v,
    )
    .expect("offset point");
    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        &ir,
        &offset,
        point,
        Some(Point2::new(0.8, expected.v)),
        Some(FIT_TOLERANCE),
    )
    .expect("global inverse fallback");

    assert!((actual.u - expected.u).abs() <= PARAMETER_TOLERANCE);
    assert!((actual.v - expected.v).abs() <= PARAMETER_TOLERANCE);

    let nested = SurfaceId::mint("synthetic:wavy-nested-offset").expect("identity grammar");
    let nested_construction =
        ProceduralSurfaceId::mint("synthetic:wavy-nested-offset-construction")
            .expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: nested.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: nested_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        nested_construction,
        ProceduralSurfaceDefinition::Offset {
            support: offset.clone(),
            distance: 0.5,
            u_sense: None,
            v_sense: None,
            support_extension: None,
            extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
            ),
        },
        None,
    ));
    let nested_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&ir),
        &nested,
        expected.u,
        expected.v,
    )
    .expect("nested offset point");
    let nested_actual = crate::decode::offset_surface_parameters_with_tolerance(
        &ir,
        &nested,
        nested_point,
        Some(Point2::new(0.8, expected.v)),
        Some(FIT_TOLERANCE),
    )
    .expect("nested global inverse fallback");
    assert!((nested_actual.u - expected.u).abs() <= PARAMETER_TOLERANCE);
    assert!((nested_actual.v - expected.v).abs() <= PARAMETER_TOLERANCE);
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = procedural.definition()
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    let owner = result
        .ir()
        .model
        .procedural_surface_owner(&procedural.id)
        .expect("offset owner");
    assert_ne!(owner, support);
    assert_eq!(&result.ir().model.faces[0].surface, owner);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    let surface_curves = crate::topology::surface_curves(&surface_curve);
    assert_eq!(surface_curves.len(), 1);
    assert_eq!(surface_curves[0].xmt, 12);
    assert_eq!(surface_curves[0].pcurve, 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.000_25, 0.000_75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn decode_lifts_pcurve_only_fin_carrier_to_its_surface() {
    let mut stream = pcurve_topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let surface_curve = stream
        .windows(4)
        .position(|window| window == [0, 137, 0, 25])
        .expect("surface curve");
    put_ref(&mut stream, surface_curve + 23, 1);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result.ir().model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| {
            result
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| &curve.id == id)
        })
        .expect("lifted carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Procedural { .. }));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { context, .. },
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("parametric surface curve");
    };
    assert_eq!(
        context.sides[0].surface,
        Some(result.ir().model.faces[0].surface.clone())
    );
    assert!(context.sides[0].pcurve.is_some());
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir().model.faces[0].surface,
        *result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
            .expect("blend owner")
    );
}

#[test]
fn decode_binds_blend_ball_centre_spine() {
    let stream = blend_surface_with_intersection_spine();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let ProceduralSurfaceDefinition::Blend { spine, .. } =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend definition");
    };
    assert_eq!(
        spine.as_ref(),
        result
            .ir()
            .model
            .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_blend_support_reference() {
    let stream = blend_surface_with_forward_blend_support();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 2);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend definition");
    };
    assert_eq!(
        supports[0].as_ref().map(|support| &support.surface),
        result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[1].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    assert_eq!(
        crate::deltas::walk(&stream).bytes_decoded,
        stream.len() - DELTAS_PREAMBLE.len()
    );
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("typed_status_framed_records")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 12.5);
    assert_eq!(result.ir().model.points[0].position.y, -2.0);
    assert_eq!(result.ir().model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 12.5);
    assert_eq!(result.ir().model.points[0].position.y, -2.0);
    assert_eq!(result.ir().model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.coedges.len(), 1);
    assert_eq!(
        result.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir().model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir().model.faces[0].surface,
        result.ir().model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_offset_surface_from_status_framed_deltas() {
    let partition = offset_surface_topology_partition_stream();
    let deltas = deltas_offset_surface_partition_stream();
    let census = crate::deltas::walk(&deltas);
    assert_eq!(census.full_counts.get("OFFSET_SURF"), Some(&1));
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::offset_surfaces(&merged)
            .iter()
            .map(|surface| surface.distance)
            .collect::<Vec<_>>(),
        [4.5]
    );
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let [procedural] = result.ir().model.procedural_surfaces.as_slice() else {
        panic!("one offset surface");
    };
    let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition() else {
        panic!("offset surface");
    };
    assert_eq!(*distance, 4.5);
    assert_eq!(
        result.ir().model.faces[0].surface,
        *result
            .ir()
            .model
            .procedural_surface_owner(&procedural.id)
            .expect("offset owner")
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_blend_surface_from_status_framed_deltas() {
    let partition = blend_surface_topology_partition_stream();
    let deltas = deltas_blend_surface_partition_stream();
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend surface");
    };
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -4.0
        }
    );
    assert_eq!(
        result.ir().model.faces[0].surface,
        *result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
            .expect("blend owner")
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_trimmed_curve_from_status_framed_deltas() {
    let partition = trimmed_topology_partition_stream();
    let deltas = deltas_trimmed_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::trimmed_curves(&merged)[0].parameters,
        [0.000_3, 0.000_7]
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.edges[0].param_range, Some([0.3, 0.7]));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_surface_curve_from_status_framed_deltas() {
    let partition = surface_curve_topology_partition_stream();
    let deltas = deltas_surface_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::surface_curves(&merged)[0].tolerance,
        0.000_02
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_derives_analytic_support_uv_without_serialized_values() {
    let stream = charted_intersection_without_uv_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            result
                .ir()
                .model
                .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
                == Some(&curve.id)
        })
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("intersection definition");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_accepts_intersection_terms_within_chart_tolerance() {
    let stream = charted_intersection_with_approximated_term_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            result
                .ir()
                .model
                .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
                == Some(&curve.id)
        })
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let partition = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = result
        .ir()
        .model
        .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
        .expect("intersection owner");
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points()[1].x, 10.0);
    assert_eq!(nurbs.knots(), [2.0, 2.0, 5.0, 5.0]);
}

#[test]
fn decode_assigns_ext11_uv_lanes_by_unique_surface_evaluation() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    let [Some(PcurveGeometry::Nurbs { nurbs: first }), Some(PcurveGeometry::Nurbs { nurbs: second })] =
        context.sides.clone().map(|side| side.pcurve)
    else {
        panic!("two ext11 pcurves");
    };
    assert_eq!(
        first.control_points(),
        [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]
    );
    assert_eq!(
        second.control_points(),
        [Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)]
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn ext11_uv_assignment_eliminates_the_complementary_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let surfaces = [
        result.ir().model.surfaces[0].id.clone(),
        result.ir().model.surfaces[1].id.clone(),
    ];
    result.ir_mut().model.surfaces[1].geometry = SurfaceGeometry::Unknown { record: None };
    let lanes = [
        Some(vec![[0.0, 0.0], [0.01, 0.0]]),
        Some(vec![[0.0, 0.0], [0.0, 0.01]]),
    ];

    let assigned = crate::decode::support_uv::assign_ext11_support_uv_to_surfaces(
        result.ir(),
        [&surfaces[0], &surfaces[1]],
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        0.01,
        &lanes,
    )
    .unwrap();

    assert_eq!(assigned, [lanes[0].clone(), None]);
}

#[test]
fn decode_replaces_ambiguous_ext11_uv_lanes_from_analytic_supports() {
    let stream = two_support_ext11_charted_intersection_curve_stream(true);
    let partition = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_completes_one_non_sentinel_ext11_uv_lane_analytically() {
    let stream = partial_ext11_charted_intersection_curve_stream();
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn completed_intersection_support_lane_attaches_after_topology_emission() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId::mint("synthetic:cube:edge#0").expect("identity grammar");
    let target_index = ir
        .model
        .coedges
        .iter()
        .position(|coedge| coedge.edge == edge && coedge.id.as_str().contains("bottom"))
        .expect("bottom coedge index");
    let target = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge && coedge.id.as_str().contains("bottom"))
        .expect("bottom coedge");
    target.id = cadmpeg_ir::ids::CoedgeId::mint("nx:s0:fin#42").expect("identity grammar");
    target.pcurves.clear();
    let owner_loop = target.owner_loop.clone();
    let surface = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == owner_loop)
        .and_then(|loop_| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support");
    let curve = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.curve.clone())
        .expect("edge curve");
    let edge_tolerance = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.tolerance);
    let _attached = ir.model.add_procedural_curve(
        curve,
        cadmpeg_ir::geometry::ProceduralCurve::new(
            cadmpeg_ir::ids::ProceduralCurveId::mint("nx:test:intersection#0")
                .expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve_parameter_range: None,
                            pcurve: Some(PcurveGeometry::Nurbs {
                                nurbs: PcurveNurbs::new(
                                    1,
                                    vec![0.0, 0.0, 1.0, 1.0],
                                    vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                    None,
                                    false,
                                )
                                .expect("valid support pcurve"),
                            }),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");
    let graph = crate::topology::Graph::parse(&[]);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(usize::MAX);

    crate::decode::support_uv::attach_completed_intersection_pcurves_for_stream_with_budget(
        &mut ir,
        &graph,
        "nx:s0",
        target_index + 1,
        0,
        source_stream,
        &mut annotations,
        &std::collections::BTreeMap::new(),
        &geometry_budget,
    );
    assert!(!ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.id.as_str().contains("intersection-pcurve-completed")));
    let source = crate::decode::support_uv::IntersectionCompletionSource {
        prefix: "nx:s0".into(),
        graph: &graph,
        source_stream,
        coedge_start: 0,
        procedural_start: 0,
    };
    crate::decode::support_uv::attach_completed_intersection_pcurves_for_model_with_budget(
        &mut ir,
        std::slice::from_ref(&source),
        &mut annotations,
        &std::collections::BTreeMap::new(),
        &geometry_budget,
    );

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str().contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert_eq!(completed.fit_tolerance(), edge_tolerance);
    assert!(ir.model.coedges.iter().any(|coedge| coedge
        .pcurves
        .iter()
        .any(|pcurve| pcurve.pcurve == completed.id)));
}

#[test]
fn linear_intersection_endpoint_witness_requires_a_clamped_linear_curve() {
    let curve_id =
        cadmpeg_ir::ids::CurveId::mint("synthetic:intersection:curve#0").expect("identity grammar");
    let first = Point3::new(1.0, 2.0, 3.0);
    let last = Point3::new(4.0, 5.0, 6.0);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![first, last],
                None,
                false,
            )
            .expect("valid clamped witness curve"),
        ),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);

    assert_eq!(
        crate::decode::pcurves::linear_nurbs_curve_endpoint_witness_with_index(&index, &curve_id),
        Some([first, last])
    );

    ir.model.curves[0].geometry = CurveGeometry::Nurbs(
        cadmpeg_ir::geometry::NurbsCurve::new(
            1,
            vec![0.0, 0.5, 1.0, 1.0],
            vec![first, last],
            None,
            false,
        )
        .expect("cardinality-valid unclamped witness curve"),
    );
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
    assert!(
        crate::decode::pcurves::linear_nurbs_curve_endpoint_witness_with_index(&index, &curve_id)
            .is_none()
    );
}

#[test]
fn ext11_uv_completion_runs_after_support_incidence_resolution() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let procedural_id = result.ir().model.procedural_curves[0].id.clone();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves[0].edit_definition(|definition| {
            let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
                definition
            else {
                panic!("typed intersection");
            };
            for side in &mut context.sides {
                side.pcurve = None;
            }
        });
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        SerializedSupportUv::from_ext11([
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ]),
    )];

    crate::decode::complete_ext11_support_uv(&mut result.ir_mut(), &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_fills_missing_intersection_support_lanes() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let procedural_id = result.ir().model.procedural_curves[0].id.clone();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                panic!("typed intersection");
            };
            for side in &mut context.sides {
                side.pcurve = None;
            }
        });
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn support_uv_completion_uses_a_finite_serialized_lane_as_a_nurbs_seed() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, NurbsSurface, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    const FIT_TOLERANCE: f64 = 1.0e-9;

    let surface_id =
        SurfaceId::mint("synthetic:serialized-seed-surface").expect("identity grammar");
    let curve_id = CurveId::mint("synthetic:serialized-seed-curve").expect("identity grammar");
    let procedural_id = ProceduralCurveId::mint("synthetic:serialized-seed-intersection")
        .expect("identity grammar");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                2,
                2,
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 10.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 10.0, 0.0),
                ],
                None,
                false,
                false,
                false,
            )
            .expect("valid serialized-seed surface"),
        ),
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        curve_id,
        ProceduralCurve::new(
            procedural_id.clone(),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(surface_id),
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );

    let parameters = [Point2::new(0.2, 0.3), Point2::new(0.7, 0.8)];
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let points = parameters
        .into_iter()
        .map(|parameter| {
            cadmpeg_ir::eval::model_surface_point_by_id(
                &index,
                &SurfaceId::mint("synthetic:serialized-seed-surface").expect("identity grammar"),
                parameter.u,
                parameter.v,
            )
            .expect("NURBS chart point")
        })
        .collect::<Vec<_>>();
    let pending = vec![(
        procedural_id,
        points,
        vec![0.0, 1.0],
        FIT_TOLERANCE,
        SerializedSupportUv::from_values([
            Some(
                parameters
                    .map(|parameter| [parameter.u, parameter.v])
                    .to_vec(),
            ),
            None,
        ]),
    )];
    let support_budget = cadmpeg_core::decode::WorkBudget::new(2);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(64);
    let coupled_support_budget = cadmpeg_core::decode::WorkBudget::new(2);

    crate::decode::support_uv::complete_support_uv_with_budget(
        &mut ir,
        &pending,
        &support_budget,
        &geometry_budget,
        &coupled_support_budget,
        &geometry_budget,
    );

    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    let Some(PcurveGeometry::Nurbs { nurbs }) = context.sides[0].pcurve.as_ref() else {
        panic!("serialized seed completed the NURBS lane");
    };
    assert_eq!(nurbs.control_points(), parameters);
}

#[test]
fn coupled_uv_completion_fills_both_missing_procedural_lanes_from_the_chart() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let base_surfaces = [
        SurfaceId::mint("synthetic:coupled-base-first").expect("identity grammar"),
        SurfaceId::mint("synthetic:coupled-base-second").expect("identity grammar"),
    ];
    let procedural_surfaces = [
        SurfaceId::mint("synthetic:coupled-procedural-first").expect("identity grammar"),
        SurfaceId::mint("synthetic:coupled-procedural-second").expect("identity grammar"),
    ];
    let constructions = [
        ProceduralSurfaceId::mint("synthetic:coupled-construction-first")
            .expect("identity grammar"),
        ProceduralSurfaceId::mint("synthetic:coupled-construction-second")
            .expect("identity grammar"),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: base_surfaces[0].clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: base_surfaces[1].clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    for (side, surface) in procedural_surfaces.iter().enumerate() {
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: constructions[side].clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            constructions[side].clone(),
            ProceduralSurfaceDefinition::Offset {
                support: base_surfaces[side].clone(),
                distance: 0.0,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                ),
            },
            None,
        ));
    }

    let procedural_id =
        ProceduralCurveId::mint("synthetic:coupled-intersection").expect("identity grammar");
    let carrier = CurveId::mint("synthetic:coupled-carrier").expect("identity grammar");
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: carrier.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        carrier,
        ProceduralCurve::new(
            procedural_id.clone(),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: procedural_surfaces
                        .clone()
                        .map(|surface| IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve_parameter_range: None,
                            pcurve: None,
                        }),
                    parameter_range: [0.0, 5.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );
    let points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 5.0),
    ];
    let parameters = vec![0.0, 2.0, 5.0];
    let pending = vec![(
        procedural_id,
        points.clone(),
        parameters.clone(),
        1.0e-3,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_coupled_support_uv_for_test(&mut ir, &pending);

    let procedural = &ir.model.procedural_curves[0];
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    for (side, surface) in procedural_surfaces.iter().enumerate() {
        for (parameter, expected) in parameters.iter().zip(&points) {
            let uv = cadmpeg_ir::eval::pcurve_uv(
                context.sides[side].pcurve.as_ref().unwrap(),
                *parameter,
            )
            .unwrap();
            let actual =
                cadmpeg_ir::eval::model_surface_point_by_id(&index, surface, uv.u, uv.v).unwrap();
            assert!((actual.x - expected.x).abs() <= 1.0e-3);
            assert!((actual.y - expected.y).abs() <= 1.0e-3);
            assert!((actual.z - expected.z).abs() <= 1.0e-3);
        }
    }
}

#[test]
fn support_uv_completion_closes_blend_spine_dependencies_to_a_fixed_point() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let spine_id = result.ir().model.procedural_curves[0].id.clone();
    let spine_curve = result
        .ir()
        .model
        .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
        .expect("spine owner")
        .clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    let spine_surfaces = context
        .sides
        .each_ref()
        .map(|side| side.surface.clone().unwrap());
    let radius = 2.0;
    let offset_surfaces = [0usize, 1usize].map(|side| {
        let support = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == spine_surfaces[side])
            .unwrap();
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } = support.geometry
        else {
            panic!("plane support");
        };
        let id =
            SurfaceId::mint(format!("synthetic:offset-support-{side}")).expect("identity grammar");
        result.ir_mut().model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(
                    origin.x + radius * normal.x,
                    origin.y + radius * normal.y,
                    origin.z + radius * normal.z,
                ),
                normal,
                u_axis,
            },
            source_object: None,
        });
        id
    });
    let blend = SurfaceId::mint("synthetic:dependent-blend").expect("identity grammar");
    let blend_construction = ProceduralSurfaceId::mint("synthetic:dependent-blend-definition")
        .expect("identity grammar");
    result.ir_mut().model.surfaces.push(Surface {
        id: blend.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: blend_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    result
        .ir_mut()
        .model
        .procedural_surfaces
        .push(ProceduralSurface::new(
            blend_construction,
            ProceduralSurfaceDefinition::Blend {
                supports: offset_surfaces.map(|surface| {
                    Some(BlendSupport {
                        surface,
                        reversed: false,
                    })
                }),
                spine: Some(spine_curve.clone()),
                radius: BlendRadiusLaw::Constant {
                    signed_radius: radius,
                },
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            None,
        ));
    let parameters = vec![0.0, 0.01];
    let spine_carrier = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == spine_curve)
        .expect("blend spine carrier");
    assert!(
        cadmpeg_ir::eval::curve_point(&spine_carrier.geometry, 0.0).is_some(),
        "spine carrier: {:?}",
        spine_carrier.geometry
    );
    let points = parameters
        .iter()
        .map(|parameter| {
            crate::decode::blend_surface_point(result.ir(), &blend, *parameter, 0.5).unwrap()
        })
        .collect::<Vec<_>>();

    let dependent_id =
        ProceduralCurveId::mint("synthetic:dependent-intersection").expect("identity grammar");
    let mut dependent = result.ir().model.procedural_curves[0].clone();
    dependent.id = dependent_id.clone();
    dependent.edit_definition(|definition| {
        let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
            unreachable!()
        };
        context.sides[0].surface = Some(blend);
        context.sides[0].pcurve = None;
        context.sides[1].surface = None;
        context.sides[1].pcurve = None;
    });
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves.insert(0, dependent);
        ir.model.procedural_curves[1].edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                unreachable!()
            };
            for side in &mut context.sides {
                side.pcurve = None;
            }
        });
    }
    let pending = vec![
        (
            dependent_id,
            points,
            parameters.clone(),
            0.01,
            SerializedSupportUv::default(),
        ),
        (
            spine_id,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            parameters,
            0.01,
            SerializedSupportUv::default(),
        ),
    ];

    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        unreachable!()
    };
    assert!(context.sides[0].pcurve.is_some());
}

#[test]
fn support_uv_completion_does_not_retry_unchanged_failed_lanes() {
    use cadmpeg_ir::ids::ProceduralCurveId;
    use cadmpeg_ir::math::Point3;

    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let template = result.ir().model.procedural_curves[0].clone();
    let mut successful = template.clone();
    let successful_id =
        ProceduralCurveId::mint("synthetic:support-uv-success").expect("identity grammar");
    successful.id = successful_id.clone();
    let mut failed = template;
    let failed_id =
        ProceduralCurveId::mint("synthetic:support-uv-failure").expect("identity grammar");
    failed.id = failed_id.clone();
    for procedural in [&mut successful, &mut failed] {
        procedural.edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                panic!("typed intersection");
            };
            context.sides[0].pcurve = None;
        });
    }
    result
        .ir_mut()
        .model
        .procedural_curves
        .extend([successful, failed]);

    let pending = vec![
        (
            successful_id,
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.01, 0.0, 0.0)],
            vec![0.0, 0.01],
            0.01,
            SerializedSupportUv::default(),
        ),
        (
            failed_id,
            vec![
                Point3::new(100.0, 100.0, 100.0),
                Point3::new(100.01, 100.0, 100.0),
            ],
            vec![0.0, 0.01],
            0.01,
            SerializedSupportUv::default(),
        ),
    ];
    let support_budget = cadmpeg_core::decode::WorkBudget::new(10);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    let coupled_support_budget = cadmpeg_core::decode::WorkBudget::new(10);
    crate::decode::support_uv::complete_support_uv_with_budget(
        &mut result.ir_mut(),
        &pending,
        &support_budget,
        &geometry_budget,
        &coupled_support_budget,
        &geometry_budget,
    );

    let successful = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.id.0 == "synthetic:support-uv-success")
        .unwrap();
    let failed = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.id.0 == "synthetic:support-uv-failure")
        .unwrap();
    let missing = |procedural: &cadmpeg_ir::geometry::ProceduralCurve| {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            panic!("typed intersection");
        };
        context.sides[0].pcurve.is_none()
    };
    assert!(!missing(successful));
    assert!(missing(failed));
    assert_eq!(support_budget.remaining(), 6);
}

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let procedural_id = result.ir().model.procedural_curves[0].id.clone();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                panic!("typed intersection");
            };
            let Some(PcurveGeometry::Nurbs { nurbs }) = context.sides[0].pcurve.as_mut() else {
                panic!("NURBS support lane");
            };
            nurbs.control_points_mut()[1] = Point2::new(
                crate::decode::MISSING_TOLERANCE,
                crate::decode::MISSING_TOLERANCE,
            );
        });
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { nurbs }) = context.sides[0].pcurve.as_ref() else {
        panic!("NURBS support lane");
    };
    assert!(nurbs.control_points().iter().all(|point| {
        point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
            && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
    }));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_replaces_a_finite_mismatched_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let procedural_id = result.ir().model.procedural_curves[0].id.clone();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                panic!("typed intersection");
            };
            let Some(PcurveGeometry::Nurbs { nurbs }) = context.sides[0].pcurve.as_mut() else {
                panic!("NURBS support lane");
            };
            for point in nurbs.control_points_mut() {
                point.u += 100.0;
            }
        });
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        SerializedSupportUv::default(),
    )];

    crate::decode::invalidate_inconsistent_support_uv(&mut result.ir_mut(), &pending);
    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let supports = [
        SurfaceId::mint("support-a").expect("identity grammar"),
        SurfaceId::mint("support-b").expect("identity grammar"),
    ];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let offsets = [
        SurfaceId::mint("offset-a").expect("identity grammar"),
        SurfaceId::mint("offset-b").expect("identity grammar"),
    ];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction = ProceduralSurfaceId::mint(format!("offset-construction-{ordinal}"))
            .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 30.0,
                u_sense: Some(0),
                v_sense: Some(0),
                support_extension: None,
                extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                ),
            },
            None,
        ));
    }
    let carrier = CurveId::mint("curve").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: carrier.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        carrier,
        ProceduralCurve::new(
            ProceduralCurveId::mint("intersection").expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[0].clone()),
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[1].clone()),
                            pcurve_parameter_range: None,
                            pcurve: Some(PcurveGeometry::Line {
                                origin: Point2::new(1.0, 2.0),
                                direction: Point2::new(3.0, 4.0),
                            }),
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );

    assert!(crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    crate::decode::complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[0].pcurve, context.sides[1].pcurve);

    ir.model.procedural_surfaces[1].edit_definition(|definition| {
        if let ProceduralSurfaceDefinition::Offset {
            support_extension, ..
        } = definition
        {
            *support_extension = Some(cadmpeg_ir::geometry::OffsetSupportExtension::Linear);
        }
    });
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    ir.model.procedural_surfaces[1].edit_definition(|definition| {
        if let ProceduralSurfaceDefinition::Offset {
            distance,
            support_extension,
            ..
        } = definition
        {
            *support_extension = None;
            *distance = 31.0;
        }
    });
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}
