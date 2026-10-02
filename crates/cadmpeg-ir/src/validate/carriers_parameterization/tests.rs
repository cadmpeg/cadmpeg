// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::parameter_in_domain;
use crate::examples::unit_cube;
use crate::geometry::{pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry};
use crate::ids::{CurveId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::report::check::Check;
use crate::test_support::make_first_face_surface_unknown;
use crate::unknown::NativeUnknownRecord;
use crate::validate::validate_neutral;

#[test]
fn parameter_domain_accepts_serialization_rounding_at_a_boundary() {
    let lower = 0.1_f64;
    let upper = std::f64::consts::TAU;
    let one_ulp_below_lower = f64::from_bits(lower.to_bits() - 1);
    let one_ulp_above_upper = f64::from_bits(upper.to_bits() + 1);

    assert!(parameter_in_domain(one_ulp_below_lower, [lower, upper]));
    assert!(parameter_in_domain(one_ulp_above_upper, [lower, upper]));
    assert!(!parameter_in_domain(lower - 1.0e-8, [lower, upper]));
    assert!(!parameter_in_domain(upper + 1.0e-8, [lower, upper]));
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId::mint("synthetic:cube:unknown#0").expect("valid identity");
    ir.set_native_unknowns(&cadmpeg_test_support::service_decode_context(),
        "synthetic",
        &[NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        }],
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.model.faces.len(), 6);
    // The situation is surfaced as a count.
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_without_record_is_legal() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    make_first_face_surface_unknown(&mut ir, None);
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(
        report.is_ok(),
        "an unknown surface need not preserve bytes, got: {:?}",
        report.findings
    );
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_dangling_record_is_flagged() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(
        &mut ir,
        Some(UnknownId::mint("test:model:entity#missing").expect("valid identity")),
    );
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding
                .message
                .contains("missing unknown record `test:model:entity#missing`")
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId::mint("test:model:entity#zz:orphan").expect("valid identity");
    ir.model.curves.push(orphan);
    assert!(validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn malformed_unknown_does_not_erase_another_records_carrier_link() {
    let mut ir = unit_cube().unwrap();
    let mut carrier = ir.model.curves[0].clone();
    carrier.id = CurveId::mint("test:model:curve#native-only").unwrap();
    let carrier_id = carrier.id.as_str().to_owned();
    ir.model.curves.push(carrier);
    ir.model.finalize(&cadmpeg_test_support::service_decode_context()).expect("fixture ordering is admitted");
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["native"] = serde_json::json!({"test": {"unknowns": [
        {"id": "test:source:unknown#good", "links": [carrier_id]},
        {"id": "test:source:unknown#malformed", "links": [null]}
    ]}});
    let parsed = crate::CadIr::from_json(&wire.to_string()).unwrap();
    let findings = validate_neutral(&parsed, Vec::new()).expect("resource allocation did not fail").findings;
    assert!(findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks
            && finding.entity.as_deref() == Some("test:source:unknown#malformed")));
    assert!(
        !findings
            .iter()
            .any(|finding| finding.check == Check::CarrierReachability
                && finding.entity.as_deref() == Some(carrier_id.as_str())),
        "{findings:?}"
    );

    wire["native"]["test"]["unknowns"][0]["links"] = serde_json::json!([]);
    let orphan = crate::CadIr::from_json(&wire.to_string()).unwrap();
    assert!(validate_neutral(&orphan, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability
            && finding.entity.as_deref() == Some(carrier_id.as_str())));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let curve_id = ir.model.edges[0].curve().cloned().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        crate::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    ir.model.edges[0].carrier =
        crate::topology::EdgeCarrier::new(ir.model.edges[0].curve().cloned(), Some([0.0, 7.0]))
            .unwrap();
    assert!(validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].carrier = crate::topology::EdgeCarrier::new(
        ir.model.edges[0].curve().cloned(),
        Some([-std::f64::consts::PI, std::f64::consts::PI]),
    )
    .unwrap();
    assert!(!validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn periodic_nurbs_rejects_an_edge_wider_than_its_large_finite_period() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let curve_id = ir.model.edges[0].curve().cloned().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        crate::geometry::nurbs::NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, f64::MAX, f64::MAX],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            true,
        ).expect("fixture constructor admission")
        .unwrap(),
    ));
    ir.model.edges[0].carrier =
        crate::topology::EdgeCarrier::new(Some(curve_id), Some([-f64::MAX, f64::MAX])).unwrap();
    assert!(validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

fn nurbs_pcurve_leaf() -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 1.0),
            ],
            None,
            false,
        ).expect("fixture pcurve construction admission")
        .unwrap(),
    }
}

fn placed_pcurve(placements: usize) -> Result<PcurveGeometry, &'static str> {
    let mut geometry = nurbs_pcurve_leaf();
    for _ in 0..placements {
        geometry = PcurveGeometry::Transformed(crate::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(geometry),
            crate::transform::Transform2::identity(),
        )?);
    }
    Ok(geometry)
}

#[test]
fn pcurve_bounded_domain_requirement_stops_at_the_admitted_nesting_depth() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = cadmpeg_core::decode::u64_from_index(crate::geometry::MAX_GEOMETRY_NESTING + 1);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let accepted = placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING).expect("admitted nesting");
    assert!(super::pcurve_requires_bounded_domain(&ctx, &accepted).unwrap());

    // Constructor admission also rejects a carrier beyond the inline bound.
    assert_eq!(
        placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING + 1),
        Err("PlacedPcurve.basis nests past the admitted inline basis depth")
    );
}

fn trimmed_over_the_nurbs_leaf() -> PcurveGeometry {
    PcurveGeometry::Trimmed(
        crate::geometry::pcurve::TrimmedPcurve::try_new(
            [0.25, 0.75],
            true,
            Box::new(nurbs_pcurve_leaf()),
        )
        .unwrap(),
    )
}

fn offset_over_the_nurbs_leaf() -> PcurveGeometry {
    PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(0.5, Box::new(nurbs_pcurve_leaf())).unwrap(),
    )
}

/// A unit cube whose bottom coedge uses one nested pcurve over `range`.
fn cube_with_one_coedge_pcurve(
    geometry: PcurveGeometry,
    range: [f64; 2],
) -> crate::document::CadIr {
    let id = crate::ids::PcurveId::mint("synthetic:cube:pcurve#nested").expect("valid identity");
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: id.clone(),
        geometry,
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    });
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: Some(crate::geometry::DirectedParameterRange::new(range).unwrap()),
    }];
    ir
}

fn coedge_pcurve_range_reported(ir: &crate::document::CadIr) -> bool {
    validate_neutral(ir, Vec::new()).expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::ParameterDomain
                && finding.message.contains("coedge pcurve range")
        })
}

#[test]
fn a_coedge_range_outside_a_trimmed_pcurve_interval_is_out_of_domain() {
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        trimmed_over_the_nurbs_leaf(),
        [0.25, 0.75]
    )));
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        trimmed_over_the_nurbs_leaf(),
        [0.0, 1.0]
    )));
}

#[test]
fn a_coedge_range_outside_an_offset_pcurve_basis_domain_is_out_of_domain() {
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over_the_nurbs_leaf(),
        [0.0, 1.0]
    )));
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over_the_nurbs_leaf(),
        [0.0, 2.0]
    )));
}

/// A NURBS pcurve whose knot vector states no evaluable interval: the
/// degree-th knot and the pole-count-th knot are equal, so
/// `nurbs_pcurve_parameter_domain` answers `None`.
fn undomained_nurbs_pcurve_leaf() -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, 0.0, 0.0],
            vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 1.0),
            ],
            None,
            false,
        ).expect("fixture pcurve construction admission")
        .unwrap(),
    }
}

fn line_pcurve_leaf() -> PcurveGeometry {
    PcurveGeometry::Line(
        crate::geometry::pcurve::LinePcurve::try_new(
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(1.0, 1.0),
        )
        .unwrap(),
    )
}

fn degenerate_trim_over(basis: PcurveGeometry) -> PcurveGeometry {
    PcurveGeometry::Trimmed(
        crate::geometry::pcurve::TrimmedPcurve::try_new([0.5, 0.5], true, Box::new(basis)).unwrap(),
    )
}

fn offset_over(basis: PcurveGeometry) -> PcurveGeometry {
    PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(0.5, Box::new(basis)).unwrap(),
    )
}

#[test]
fn a_bare_nurbs_pcurve_with_no_domain_requires_one() {
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        undomained_nurbs_pcurve_leaf(),
        [0.0, 1.0]
    )));
}

#[test]
fn a_trim_and_an_offset_inherit_the_basis_bounded_domain_requirement() {
    // The basis states no domain and must: the same NURBS reported bare is
    // reported under a degenerate trim and under an offset.
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        degenerate_trim_over(undomained_nurbs_pcurve_leaf()),
        [0.0, 1.0]
    )));
    assert!(coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over(undomained_nurbs_pcurve_leaf()),
        [0.0, 1.0]
    )));

    // A line states no domain and needs none, so neither wrapper over it
    // requires one.
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        degenerate_trim_over(line_pcurve_leaf()),
        [0.0, 1.0]
    )));
    assert!(!coedge_pcurve_range_reported(&cube_with_one_coedge_pcurve(
        offset_over(line_pcurve_leaf()),
        [0.0, 1.0]
    )));
}

#[test]
fn numerical_audit_parameter_domain_roundoff_is_relative_to_width() {
    for d in [1., 1e-16] {
        assert!(super::parameter_in_domain(0., [0., d]));
        assert!(super::parameter_in_domain(d, [0., d]));
        assert!(!super::parameter_in_domain(2. * d, [0., d]));
    }
}

#[test]
fn pcurve_requires_bounded_domain_preserves_session_depth_and_work_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let geometry = placed_pcurve(2).unwrap();
    for (dimension, cap, held_frame) in [
        (ResourceDimension::RecursionDepth, 0, false),
        (ResourceDimension::RecursionDepth, 2, false),
        (ResourceDimension::RecursionDepth, 2, true),
        (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 2, false),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let guard = held_frame.then(|| ctx.enter_nested_limit("caller frame").unwrap());
        let limit = super::pcurve_requires_bounded_domain(&ctx, &geometry).unwrap_err();
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, cap);
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.operation, match dimension {
            ResourceDimension::RecursionDepth => "pcurve bounded domain nesting",
            _ => "pcurve bounded domain visit",
        });
        drop(guard);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn parameter_domain_indexes_preserve_resource_refusals_and_release_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut ir = unit_cube().unwrap();
    ir.model.edges.clear();
    ir.model.coedges.clear();
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = super::check_parameter_domains(&ctx, &ir, &mut findings) else { panic!("domain index must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 8192;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_parameter_domains(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(8192, "domain indexes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn carrier_law_walk_preserves_first_later_and_active_session_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use crate::geometry::LawExpression;
    let expression: LawExpression = LawExpression::Algebraic { operator: "outer".into(), operands: vec![
        LawExpression::Algebraic { operator: "inner".into(), operands: vec![LawExpression::Null {}] },
    ] };
    for (dimension, cap, active) in [
        (ResourceDimension::RecursionDepth, 0, false), (ResourceDimension::RecursionDepth, 2, false),
        (ResourceDimension::RecursionDepth, 2, true), (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 2, false),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut index = super::BorrowedIdentities::build(&ctx, |_| Ok(())).unwrap();
        let caller = if active { Some(ctx.enter_nested("carrier law caller").unwrap()) } else { None };
        let Err(CodecError::ResourceLimit(limit)) = super::collect_law_curves(&expression, &mut index, &ctx) else { panic!("law walk must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, if dimension == ResourceDimension::RecursionDepth { "carrier law nesting" } else { "carrier law visit" });
        assert!(index.identities().next().is_none());
        drop(caller);
        drop(index);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn carrier_reachability_preserves_index_scan_and_finding_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut ir = unit_cube().unwrap();
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = "test:model:curve#orphan".try_into().unwrap();
    orphan.source_object = None;
    ir.model.curves.push(orphan);
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = super::check_carrier_reachability(&ctx, crate::native::view::NativeView::new(&ir, None), &mut findings) else { panic!("carrier check must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn carrier_reachability_expands_composite_cycles_once_and_releases_scopes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::geometry::{CompositeCurveSegment, CompositeCurveTransition};
    let mut ir = unit_cube().unwrap();
    let root = ir.model.curves[0].id.clone();
    let mut first = ir.model.curves[0].clone();
    first.id = "test:model:curve#first".try_into().unwrap();
    first.source_object = None;
    let mut second = first.clone();
    second.id = "test:model:curve#second".try_into().unwrap();
    let segment = |curve| CompositeCurveSegment { curve, same_sense: true, transition: CompositeCurveTransition::Discontinuous };
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![segment(first.id.clone()), segment(first.id.clone()), segment(second.id.clone())].try_into().unwrap(), self_intersect: None,
    });
    first.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![segment(root)].try_into().unwrap(), self_intersect: None,
    });
    second.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![segment(first.id.clone())].try_into().unwrap(), self_intersect: None,
    });
    ir.model.curves.extend([first, second]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_carrier_reachability(&ctx, crate::native::view::NativeView::new(&ir, None), &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(4096, "carrier indexes and queue released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn carrier_reachability_preserves_orphan_arena_order() {
    let mut ir = unit_cube().unwrap();
    let mut surface = ir.model.surfaces[0].clone();
    surface.id = "test:model:surface#orphan".try_into().unwrap();
    surface.source_object = None;
    ir.model.surfaces.push(surface);
    let mut curve = ir.model.curves[0].clone();
    curve.id = "test:model:curve#orphan".try_into().unwrap();
    curve.source_object = None;
    ir.model.curves.push(curve);
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: "test:model:pcurve#orphan".try_into().unwrap(), geometry: line_pcurve_leaf(), metadata: Default::default(),
    });
    let mut point = ir.model.points[0].clone();
    point.id = "test:model:point#orphan".try_into().unwrap();
    point.source_object = None;
    ir.model.points.push(point);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_carrier_reachability(&ctx, crate::native::view::NativeView::new(&ir, None), &mut findings).unwrap();
    assert_eq!(findings.len(), 4);
    for (finding, kind) in findings.iter().zip(["surface", "curve", "pcurve", "point"]) {
        assert_eq!(finding.check, Check::CarrierReachability);
        assert_eq!(finding.severity, crate::report::Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(format!("test:model:{kind}#orphan").as_str()));
        assert_eq!(finding.message, format!("orphan {kind} carrier"));
    }
}
