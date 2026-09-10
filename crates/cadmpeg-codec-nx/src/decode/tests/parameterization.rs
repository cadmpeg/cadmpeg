// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::decode::blend::blend_surface_point;
use crate::decode::offset::{offset_surface_parameters, offset_surface_parameters_with_tolerance};
use crate::decode::support_uv::{
    complete_ext11_support_uv, complete_parameterization_equivalent_support_uv,
    invalidate_inconsistent_support_uv, parameterization_equivalent_surfaces, SerializedSupportUv,
};

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, PcurveGeometry, PcurveNurbs,
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

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

    let actual = offset_surface_parameters(result.ir(), &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let mut translated = result.ir().clone();
    for carrier in &mut translated.model.surfaces {
        if let SurfaceGeometry::Plane(plane_surface) = &mut carrier.geometry {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            let mut origin = *origin;
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
            *plane_surface =
                cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
        }
    }
    let translated_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let translated_parameters = offset_surface_parameters_with_tolerance(
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
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#synthetic:nested-offset")
            .expect("identity grammar");
    let nested_construction = cadmpeg_ir::ids::ProceduralSurfaceId::mint(
        "test:model:entity#synthetic:nested-offset-construction",
    )
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
    translated.model.procedural_surfaces.push(
        cadmpeg_ir::geometry::ProceduralSurface::new(
            nested_construction,
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    surface,
                    -0.75,
                    None,
                    None,
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );
    let nested_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &nested_surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let nested_parameters = offset_surface_parameters_with_tolerance(
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

    let actual = offset_surface_parameters_with_tolerance(
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

    let support =
        SurfaceId::mint("test:model:entity#synthetic:wavy-support").expect("identity grammar");
    let offset =
        SurfaceId::mint("test:model:entity#synthetic:wavy-offset").expect("identity grammar");
    let construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:wavy-offset-construction")
            .expect("identity grammar");
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
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    support,
                    0.75,
                    None,
                    None,
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );

    let expected = Point2::new(0.2, 0.45);
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&ir),
        &offset,
        expected.u,
        expected.v,
    )
    .expect("offset point");
    let actual = offset_surface_parameters_with_tolerance(
        &ir,
        &offset,
        point,
        Some(Point2::new(0.8, expected.v)),
        Some(FIT_TOLERANCE),
    )
    .expect("global inverse fallback");

    assert!((actual.u - expected.u).abs() <= PARAMETER_TOLERANCE);
    assert!((actual.v - expected.v).abs() <= PARAMETER_TOLERANCE);

    let nested = SurfaceId::mint("test:model:entity#synthetic:wavy-nested-offset")
        .expect("identity grammar");
    let nested_construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:wavy-nested-offset-construction")
            .expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: nested.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: nested_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
            nested_construction,
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    offset.clone(),
                    0.5,
                    None,
                    None,
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );
    let nested_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&ir),
        &nested,
        expected.u,
        expected.v,
    )
    .expect("nested offset point");
    let nested_actual = offset_surface_parameters_with_tolerance(
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
    let ProceduralSurfaceDefinition::Offset(definition_payload) = procedural.definition() else {
        panic!("offset definition");
    };
    let support = definition_payload.support();
    let distance = definition_payload.distance();
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
    assert_eq!(surface_curves[0].state.pcurve(), 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].state.parameters(), [0.000_25, 0.000_75]);

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
        .curve()
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
        context.sides()[0].surface,
        Some(result.ir().model.faces[0].surface.clone())
    );
    assert!(context.sides()[0].pcurve.is_some());
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

    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend definition");
    };
    let spine = definition_payload.spine();

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
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend definition");
    };
    let supports = definition_payload.supports();

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
    assert!(matches!(
        carrier.geometry.solved_cache(),
        Some(CurveGeometry::Nurbs(_))
    ));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("intersection definition");
    };
    assert!(context.sides()[0].pcurve.is_some());
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
    assert!(matches!(
        carrier.geometry.solved_cache(),
        Some(CurveGeometry::Nurbs(_))
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
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
        context
            .sides()
            .clone()
            .map(|side| side.pcurve.map(|binding| binding.geometry))
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
        crate::intersection::SupportUvLane::new(vec![[0.0, 0.0], [0.01, 0.0]], 2),
        crate::intersection::SupportUvLane::new(vec![[0.0, 0.0], [0.0, 0.01]], 2),
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
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
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
    assert!(context.sides()[0].pcurve.is_some());
    assert!(context.sides()[1].pcurve.is_some());
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
        .and_then(|edge| edge.curve().clone())
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
                context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                    [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: Some(
                                PcurveGeometry::Nurbs {
                                    nurbs: PcurveNurbs::new(
                                        1,
                                        vec![0.0, 0.0, 1.0, 1.0],
                                        vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                        None,
                                        false,
                                    )
                                    .expect("valid support pcurve"),
                                }
                                .into(),
                            ),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");
    let graph = crate::topology::Graph::parse(&[]);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(usize::MAX);

    crate::decode::support_uv::attach_completed_intersection_pcurves_for_stream_with_budget(
        &mut ir,
        &graph,
        &crate::decode::ids::IdScope::stream(0),
        target_index + 1,
        0,
        source_stream.clone(),
        &mut annotations,
        &std::collections::BTreeMap::new(),
        &geometry_budget,
    )
    .expect("valid exactness fields");
    assert!(!ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.id.as_str().contains("intersection-pcurve-completed")));
    let source = crate::decode::support_uv::IntersectionCompletionSource {
        scope: crate::decode::ids::IdScope::stream(0),
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
    )
    .expect("valid exactness fields");

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str().contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert_eq!(
        completed.fit_tolerance(),
        edge_tolerance.map(cadmpeg_ir::units::PositiveScalar::get)
    );
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
        ir.model.procedural_curves[0]
            .edit_definition(|definition| {
                let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context, ..
                } = definition
                else {
                    panic!("typed intersection");
                };
                context
                    .edit(|context_sides, _, _| {
                        for side in &mut (*context_sides) {
                            side.pcurve = None;
                        }
                    })
                    .unwrap();
            })
            .unwrap();
    }
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            vec![0.0, 0.01],
        )
        .unwrap(),
        0.01,
        SerializedSupportUv::from_ext11([
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ]),
    )];

    complete_ext11_support_uv(&mut result.ir_mut(), &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
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
        ir.model.procedural_curves[0]
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                context
                    .edit(|context_sides, _, _| {
                        for side in &mut (*context_sides) {
                            side.pcurve = None;
                        }
                    })
                    .unwrap();
            })
            .unwrap();
    }
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            vec![0.0, 0.01],
        )
        .unwrap(),
        0.01,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
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

    let surface_id = SurfaceId::mint("test:model:entity#synthetic:serialized-seed-surface")
        .expect("identity grammar");
    let curve_id = CurveId::mint("test:model:entity#synthetic:serialized-seed-curve")
        .expect("identity grammar");
    let procedural_id =
        ProceduralCurveId::mint("test:model:entity#synthetic:serialized-seed-intersection")
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
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        curve_id,
        ProceduralCurve::new(
            procedural_id.clone(),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(surface_id),
                            pcurve: None,
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );

    let parameters = [Point2::new(0.2, 0.3), Point2::new(0.7, 0.8)];
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let points = parameters
        .into_iter()
        .map(|parameter| {
            cadmpeg_ir::eval::model_surface_point_by_id(
                &index,
                &SurfaceId::mint("test:model:entity#synthetic:serialized-seed-surface")
                    .expect("identity grammar"),
                parameter.u,
                parameter.v,
            )
            .expect("NURBS chart point")
        })
        .collect::<Vec<_>>();
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(points, vec![0.0, 1.0])
            .unwrap(),
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
    let Some(support) = context.sides()[0].pcurve.as_ref() else {
        panic!("serialized seed completed the NURBS lane");
    };
    let PcurveGeometry::Nurbs { nurbs } = &support.geometry else {
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
        SurfaceId::mint("test:model:entity#synthetic:coupled-base-first")
            .expect("identity grammar"),
        SurfaceId::mint("test:model:entity#synthetic:coupled-base-second")
            .expect("identity grammar"),
    ];
    let procedural_surfaces = [
        SurfaceId::mint("test:model:entity#synthetic:coupled-procedural-first")
            .expect("identity grammar"),
        SurfaceId::mint("test:model:entity#synthetic:coupled-procedural-second")
            .expect("identity grammar"),
    ];
    let constructions = [
        ProceduralSurfaceId::mint("test:model:entity#synthetic:coupled-construction-first")
            .expect("identity grammar"),
        ProceduralSurfaceId::mint("test:model:entity#synthetic:coupled-construction-second")
            .expect("identity grammar"),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: base_surfaces[0].clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: base_surfaces[1].clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
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
        ir.model.procedural_surfaces.push(
            ProceduralSurface::new(
                constructions[side].clone(),
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        base_surfaces[side].clone(),
                        0.0,
                        None,
                        None,
                        false,
                        cadmpeg_ir::geometry::OffsetExtension::Legacy(
                            cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                        ),
                    )
                    .unwrap(),
                ),
                None,
            )
            .unwrap(),
        );
    }

    let procedural_id = ProceduralCurveId::mint("test:model:entity#synthetic:coupled-intersection")
        .expect("identity grammar");
    let carrier =
        CurveId::mint("test:model:entity#synthetic:coupled-carrier").expect("identity grammar");
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
                context: IntcurveSupportContext::try_new(
                    procedural_surfaces
                        .clone()
                        .map(|surface| IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: None,
                        }),
                    [0.0, 5.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    let points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 5.0),
    ];
    let parameters = vec![0.0, 2.0, 5.0];
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            points.clone(),
            parameters.clone(),
        )
        .unwrap(),
        1.0e-3,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_coupled_support_uv_for_test(&mut ir, &pending);

    let procedural = &ir.model.procedural_curves[0];
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection");
    };
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    for (side, surface) in procedural_surfaces.iter().enumerate() {
        for (parameter, expected) in parameters.iter().zip(&points) {
            let uv = cadmpeg_ir::eval::pcurve_uv(
                &context.sides()[side].pcurve.as_ref().unwrap().geometry,
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
        .sides()
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
        let SurfaceGeometry::Plane(plane_surface) = support.geometry else {
            panic!("plane support");
        };
        let origin = *plane_surface.origin();
        let normal = *plane_surface.normal();
        let u_axis = *plane_surface.u_axis();
        let id = SurfaceId::mint(format!("test:model:entity#synthetic:offset-support-{side}"))
            .expect("identity grammar");
        result.ir_mut().model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(
                        origin.x + radius * normal.x,
                        origin.y + radius * normal.y,
                        origin.z + radius * normal.z,
                    ),
                    normal,
                    u_axis,
                )
                .unwrap(),
            ),
            source_object: None,
        });
        id
    });
    let blend =
        SurfaceId::mint("test:model:entity#synthetic:dependent-blend").expect("identity grammar");
    let blend_construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:dependent-blend-definition")
            .expect("identity grammar");
    result.ir_mut().model.surfaces.push(Surface {
        id: blend.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: blend_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    result.ir_mut().model.procedural_surfaces.push(
        ProceduralSurface::new(
            blend_construction,
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    offset_surfaces.map(|surface| {
                        Some(BlendSupport {
                            surface,
                            reversed: false,
                        })
                    }),
                    Some(spine_curve.clone()),
                    BlendRadiusLaw::Constant {
                        signed_radius: radius,
                    },
                    BlendCrossSection::Circular,
                    None,
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );
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
        .map(|parameter| blend_surface_point(result.ir(), &blend, *parameter, 0.5).unwrap())
        .collect::<Vec<_>>();

    let dependent_id =
        ProceduralCurveId::mint("test:model:entity#synthetic:dependent-intersection")
            .expect("identity grammar");
    let mut dependent = result.ir().model.procedural_curves[0].clone();
    dependent.id = dependent_id.clone();
    dependent
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                unreachable!()
            };
            context
                .edit(|context_sides, _, _| {
                    (*context_sides)[0].surface = Some(blend);
                    (*context_sides)[0].pcurve = None;
                    (*context_sides)[1].surface = None;
                    (*context_sides)[1].pcurve = None;
                })
                .unwrap();
        })
        .unwrap();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves.insert(0, dependent);
        ir.model.procedural_curves[1]
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    unreachable!()
                };
                context
                    .edit(|context_sides, _, _| {
                        for side in &mut (*context_sides) {
                            side.pcurve = None;
                        }
                    })
                    .unwrap();
            })
            .unwrap();
    }
    let pending = vec![
        (
            dependent_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                points,
                parameters.clone(),
            )
            .unwrap(),
            0.01,
            SerializedSupportUv::default(),
        ),
        (
            spine_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                vec![
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
                ],
                parameters,
            )
            .unwrap(),
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
    assert!(context.sides()[0].pcurve.is_some());
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
    let successful_id = ProceduralCurveId::mint("test:model:entity#synthetic:support-uv-success")
        .expect("identity grammar");
    successful.id = successful_id.clone();
    let mut failed = template;
    let failed_id = ProceduralCurveId::mint("test:model:entity#synthetic:support-uv-failure")
        .expect("identity grammar");
    failed.id = failed_id.clone();
    for procedural in [&mut successful, &mut failed] {
        procedural
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                context
                    .edit(|context_sides, _, _| {
                        (*context_sides)[0].pcurve = None;
                    })
                    .unwrap();
            })
            .unwrap();
    }
    {
        let mut ir = result.ir_mut();
        let owner = ir
            .model
            .procedural_curve_owner(&ir.model.procedural_curves[0].id)
            .unwrap();
        let template = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *owner)
            .unwrap()
            .clone();
        for (name, procedural) in [("success", successful), ("failure", failed)] {
            let mut carrier = template.clone();
            carrier.id =
                cadmpeg_ir::ids::CurveId::mint(format!("test:model:curve#support-uv-{name}"))
                    .unwrap();
            let CurveGeometry::Procedural { construction, .. } = &mut carrier.geometry else {
                panic!("procedural carrier");
            };
            *construction = procedural.id.clone();
            ir.model.curves.push(carrier);
            ir.model.procedural_curves.push(procedural);
        }
    }

    let pending = vec![
        (
            successful_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.01, 0.0, 0.0)],
                vec![0.0, 0.01],
            )
            .unwrap(),
            0.01,
            SerializedSupportUv::default(),
        ),
        (
            failed_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                vec![
                    Point3::new(100.0, 100.0, 100.0),
                    Point3::new(100.01, 100.0, 100.0),
                ],
                vec![0.0, 0.01],
            )
            .unwrap(),
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
        .find(|procedural| {
            procedural.id.as_str() == "test:model:entity#synthetic:support-uv-success"
        })
        .unwrap();
    let failed = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| {
            procedural.id.as_str() == "test:model:entity#synthetic:support-uv-failure"
        })
        .unwrap();
    let missing = |procedural: &cadmpeg_ir::geometry::ProceduralCurve| {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            panic!("typed intersection");
        };
        context.sides()[0].pcurve.is_none()
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
        ir.model.procedural_curves[0]
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                context
                    .edit(|context_sides, _, _| {
                        let Some(support) = (*context_sides)[0].pcurve.as_mut() else {
                            panic!("NURBS support lane");
                        };
                        let PcurveGeometry::Nurbs { nurbs } = &mut support.geometry else {
                            panic!("NURBS support lane");
                        };
                        nurbs
                            .edit_control_points(|points| {
                                points[1] = Point2::new(
                                    crate::decode::MISSING_TOLERANCE,
                                    crate::decode::MISSING_TOLERANCE,
                                );
                            })
                            .unwrap();
                    })
                    .unwrap();
            })
            .unwrap();
    }
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            vec![0.0, 0.01],
        )
        .unwrap(),
        0.01,
        SerializedSupportUv::default(),
    )];

    crate::decode::support_uv::complete_support_uv(&mut result.ir_mut(), &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("typed intersection");
    };
    let Some(support) = context.sides()[0].pcurve.as_ref() else {
        panic!("NURBS support lane");
    };
    let PcurveGeometry::Nurbs { nurbs } = &support.geometry else {
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
        ir.model.procedural_curves[0]
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                context
                    .edit(|context_sides, _, _| {
                        let Some(support) = (*context_sides)[0].pcurve.as_mut() else {
                            panic!("NURBS support lane");
                        };
                        let PcurveGeometry::Nurbs { nurbs } = &mut support.geometry else {
                            panic!("NURBS support lane");
                        };
                        nurbs
                            .edit_control_points(|points| {
                                for point in points {
                                    point.u += 100.0;
                                }
                            })
                            .unwrap();
                    })
                    .unwrap();
            })
            .unwrap();
    }
    let pending = vec![(
        procedural_id,
        crate::intersection::chart_samples::ChartSamples::from_test_values(
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            vec![0.0, 0.01],
        )
        .unwrap(),
        0.01,
        SerializedSupportUv::default(),
    )];

    invalidate_inconsistent_support_uv(&mut result.ir_mut(), &pending);
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
        SurfaceId::mint("test:model:entity#support-a").expect("identity grammar"),
        SurfaceId::mint("test:model:entity#support-b").expect("identity grammar"),
    ];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        });
    }
    let offsets = [
        SurfaceId::mint("test:model:entity#offset-a").expect("identity grammar"),
        SurfaceId::mint("test:model:entity#offset-b").expect("identity grammar"),
    ];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction =
            ProceduralSurfaceId::mint(format!("test:model:entity#offset-construction-{ordinal}"))
                .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(
            ProceduralSurface::new(
                construction,
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        support.clone(),
                        30.0,
                        Some(0),
                        Some(0),
                        false,
                        cadmpeg_ir::geometry::OffsetExtension::Legacy(
                            cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                        ),
                    )
                    .unwrap(),
                ),
                None,
            )
            .unwrap(),
        );
    }
    let carrier = CurveId::mint("test:model:entity#curve").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: carrier.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        carrier,
        ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:entity#intersection").expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                    [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[0].clone()),
                            pcurve: None,
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[1].clone()),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::LinePcurve::try_new(
                                        Point2::new(1.0, 2.0),
                                        Point2::new(3.0, 4.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );

    assert!(parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides()[0].pcurve, context.sides()[1].pcurve);

    ir.model.procedural_surfaces[1]
        .edit_definition(|definition| {
            if let ProceduralSurfaceDefinition::Offset(definition_payload) = definition {
                definition_payload.set_linear_support_extension(true);
            }
        })
        .unwrap();
    assert!(!parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    ir.model.procedural_surfaces[1]
        .edit_definition(|definition| {
            if let ProceduralSurfaceDefinition::Offset(definition_payload) = definition {
                definition_payload.set_linear_support_extension(false);
                definition_payload.try_set_distance(31.0).unwrap();
            }
        })
        .unwrap();
    assert!(!parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}

mod deltas;
