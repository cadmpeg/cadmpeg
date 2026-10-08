// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use cadmpeg_core::decode::ResourceDimension;

use super::{retain_live_annotations, unknown_stream_metadata};

fn geometry_route_limit_error(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |scan_ctx| {
            let scan_root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(scan_ctx, scan_root).expect("valid topology container");
            let (dialects, _) = crate::dialect::classify_layers(scan_ctx, &scan)
                .expect("classified topology input")
                .into_report_parts();

            crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
                match super::try_decode_geometry(ctx, &scan, &dialects, &[], &[], &mut 0) {
                    Err(error) => error,
                    Ok(_) => panic!("geometry route must refuse the low limit"),
                }
            })
        },
    )
}

#[test]
fn geometry_route_refuses_collection_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn geometry_route_refuses_retained_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn geometry_route_refuses_scoped_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_materialized_bytes = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn geometry_route_refuses_work_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
    ));
}

#[test]
fn unknown_carrier_reachability_pass_refuses_session_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx unknown carrier reachability pass",
        |ctx| {
            super::prune_unreferenced_unknown_carriers(
                ctx,
                &mut cadmpeg_ir::document::CadIr::empty(),
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "nx unknown carrier reachability pass"
    ));
}

#[test]
fn inactive_geometry_reachability_pass_refuses_session_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx active geometry reachability pass",
        |ctx| super::prune_inactive_geometry(ctx, &mut cadmpeg_ir::document::CadIr::empty()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "nx active geometry reachability pass"
    ));
}

#[test]
fn geometry_carrier_mapping_pass_refuses_session_work_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );
    let error = crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |scan_ctx| {
            let scan =
                crate::decode::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                    .expect("valid topology container");
            let (dialects, _) = crate::dialect::classify_layers(scan_ctx, &scan)
                .expect("classified topology input")
                .into_report_parts();
            crate::test_support::resource_refusal_at(
                &bytes,
                ResourceDimension::WorkUnits,
                "nx geometry carrier mapping pass",
                |ctx| {
                    super::try_decode_geometry(ctx, &scan, &dialects, &[], &[], &mut 0).map(|_| ())
                },
            )
        },
    );

    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "nx geometry carrier mapping pass"
    ));
}

fn preview_stream() -> crate::parasolid::Stream {
    crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Preview,
    }
}

fn preview_unknown() -> cadmpeg_ir::unknown::UnknownRecord {
    crate::test_support::with_decode_context(|ctx| {
        unknown_stream_metadata(ctx, 0, &preview_stream())
            .expect("preview metadata fits the service profile")
    })
}

#[test]
fn live_annotations_refuse_first_identity_at_collection_limit() {
    let unknown = preview_unknown();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = retain_live_annotations(
                ctx,
                &cadmpeg_ir::document::CadIr::empty(),
                &[unknown],
                &mut cadmpeg_ir::Annotations::default(),
            )
            .expect_err("one live identity needs one slot");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx live annotation identities"
            ));
        },
    );
}

#[test]
fn rmfastload_stream_index_parse_propagates_work_refusal() {
    let body = cadmpeg_ir::ids::BodyId::mint("nx:s3:body#selected").unwrap();
    let selected = std::collections::BTreeSet::from([body]);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx rmfastload stream index",
        |ctx| super::rmfastload_stream_indices(ctx, &selected),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("stream parsing must propagate the work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "nx rmfastload stream index");
}


fn unchanged_carrier_mapping(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use std::collections::{BTreeMap, BTreeSet};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, Surface, SurfaceGeometry, SolvedSurfaceGeometry};
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
    use cadmpeg_ir::ids::{CurveId, PcurveId, SurfaceId};
    use cadmpeg_ir::math::Point2;

    let curve = CurveId::mint("nx:test:curve#basis").unwrap();
    let pcurve = PcurveId::mint("nx:test:pcurve#basis").unwrap();
    let surface = SurfaceId::mint("nx:test:surface#support").unwrap();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    ir.model.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Line(LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap()),
        metadata: PcurveMetadata::default(),
    });
    let trims = [crate::topology::TrimmedCurve {
        xmt: 3,
        state: serde_json::from_value(serde_json::json!({
            "basis_xmt": 2, "points": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], "parameters": [0.0, 1.0],
        })).unwrap(),
        pos: 0,
    }];
    let surface_curves = [crate::topology::SurfaceCurve {
        xmt: 4,
        state: serde_json::from_value(serde_json::json!({
            "surface_xmt": 6, "pcurve_xmt": 2, "original_curve_xmt": 2,
            "tolerance_to_original": 0.0,
        })).unwrap(),
        pos: 0,
    }];
    let mut curves = BTreeMap::from([(2, curve.clone()), (3, curve.clone()), (4, curve.clone())]);
    let mut pcurves = BTreeMap::from([(2, pcurve.clone()), (3, pcurve.clone()), (4, pcurve.clone())]);
    let mut supports = BTreeMap::from([(2, surface.clone()), (3, surface.clone()), (4, surface.clone())]);
    let surfaces = BTreeMap::from([(6, surface.clone())]);
    let mut ranges = BTreeMap::from([(3, [0.0, 1.0])]);
    let mut invalid = BTreeSet::new();
    let mut storage = ctx.reserve_scoped(0, "test carrier maps")?;
    let mut invalid_storage = ctx.reserve_scoped(0, "test invalid pcurves")?;
    super::map_stream_carriers(ctx, &mut storage, &mut invalid_storage, &mut ir,
        &trims, &surface_curves, super::CarrierMaps {
            surfaces: &surfaces, curves: &mut curves, pcurves: &mut pcurves,
            pcurve_supports: &mut supports, trim_ranges: &mut ranges,
        }, &mut invalid)?;
    assert!(invalid.is_empty());
    assert_eq!(curves.get(&3), Some(&curve));
    assert_eq!(curves.get(&4), Some(&curve));
    assert_eq!(pcurves.get(&3), Some(&pcurve));
    assert_eq!(pcurves.get(&4), Some(&pcurve));
    assert_eq!(supports.get(&3), Some(&surface));
    assert_eq!(supports.get(&4), Some(&surface));
    storage.commit_value((curves, pcurves, supports, ranges))?;
    Ok(())
}

#[test]
fn unchanged_carrier_mapping_makes_no_retained_identity_copy() {
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        unchanged_carrier_mapping(ctx).unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

fn carrier_identity_refusal(operation: &str) {
    let error = crate::test_support::resource_refusal_at(&[], ResourceDimension::WorkUnits,
        operation, unchanged_carrier_mapping);
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("carrier identity comparison preserves the resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
}

#[test]
fn trimmed_curve_identity_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx trimmed curve identity comparison");
}
#[test]
fn trimmed_pcurve_identity_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx trimmed pcurve identity comparison");
}
#[test]
fn trimmed_pcurve_support_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx trimmed pcurve support comparison");
}
#[test]
fn surface_pcurve_identity_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx surface pcurve identity comparison");
}
#[test]
fn surface_pcurve_support_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx surface pcurve support comparison");
}
#[test]
fn surface_curve_identity_comparison_refuses_before_comparison() {
    carrier_identity_refusal("nx surface curve identity comparison");
}
