// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::topology::BodyKind;

struct ProjectionInput {
    directory: Vec<crate::directory::DirectoryEntry>,
    records: Vec<crate::parameter::ParameterRecord>,
    global: crate::global::ProjectedGlobal,
    ir: CadIr,
}

fn projection_input(bytes: &[u8]) -> ProjectionInput {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves = decoded.ir().model.curves.clone();
    ir.model.surfaces = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.id.as_str() != "iges:model:surface#D9")
        .cloned()
        .collect();
    ir.model.procedural_surfaces = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .filter(|construction| {
            ir.model
                .surfaces
                .iter()
                .any(|surface| surface.geometry.procedural_construction() == Some(&construction.id))
        })
        .cloned()
        .collect();
    ir.model.procedural_curves = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .filter(|construction| {
            ir.model
                .curves
                .iter()
                .any(|curve| curve.geometry.procedural_construction() == Some(&construction.id))
        })
        .cloned()
        .collect();
    ir.model.points = decoded
        .ir()
        .model
        .points
        .iter()
        .filter(|point| !point.id.as_str().starts_with("iges:model:point#D9:"))
        .cloned()
        .collect();
    ir.model.vertices = decoded
        .ir()
        .model
        .vertices
        .iter()
        .filter(|vertex| !vertex.id.as_str().starts_with("iges:model:vertex#D9:"))
        .cloned()
        .collect();
    ir.model.edges = decoded
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| !edge.id.as_str().starts_with("iges:model:edge#D9:"))
        .cloned()
        .collect();
    ir.model.bodies = decoded
        .ir()
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Wire)
        .cloned()
        .collect();
    ir.model.regions = decoded
        .ir()
        .model
        .regions
        .iter()
        .filter(|region| ir.model.bodies.iter().any(|body| body.id == region.body))
        .cloned()
        .collect();
    ir.model.shells = decoded
        .ir()
        .model
        .shells
        .iter()
        .filter(|shell| {
            ir.model
                .regions
                .iter()
                .any(|region| region.id == shell.region)
        })
        .cloned()
        .collect();
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        report.is_ok(),
        "source projection findings: {:?}",
        report.findings
    );
    crate::test_support::with_service_context(bytes, |ctx| {
        let scan = crate::card::scan_with_context(bytes, ctx).unwrap();
        let (global, _, _storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        let parameters =
            crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, ctx)
                .unwrap();
        ProjectionInput {
            directory,
            records: parameters.records,
            global: global.length_context().unwrap(),
            ir,
        }
    })
}

#[test]
fn rejected_alternate_pcurves_retain_no_mapped_knots_or_poles() {
    let mut input = projection_input(&subrange_nurbs_surface_boundary_file(2));
    let mut retained = Vec::new();
    for count in [3_usize, 512] {
        let points: Vec<_> = (0..count)
            .map(|index| {
                FinitePoint3::new(Point3::new(
                    if index == count / 2 { 0.1 } else { 0.2 },
                    0.2,
                    0.0,
                ))
                .unwrap()
            })
            .collect();
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..count).map(|index| {
            f64::from(u32::try_from(index).unwrap()) / f64::from(u32::try_from(count - 1).unwrap())
        }));
        knots.push(1.0);
        let nurbs = crate::test_support::with_service_context(&[], |ctx| {
            NurbsCurve::new(ctx, 1, knots, NurbsPoles3::Polynomial { points }, false)
                .unwrap()
                .unwrap()
        });
        input
            .ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
            .unwrap()
            .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "test retained alternate output total",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = 1024 * 1024;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ir = input.ir.clone();
                let mut derivation_storage = ctx
                    .reserve_scoped(0, "test alternate pcurve derivations")
                    .unwrap();
                let mut sequences = super::super::super::geometry::SourceSequences::default();
                let (projection, derivations) = super::super::project(
                    &mut ir,
                    &input.directory,
                    &input.records,
                    &input.global,
                    (&ctx, &mut derivation_storage),
                    &mut sequences,
                )
                .unwrap();
                assert!(projection.decoded.contains(&9));
                assert!(
                    projection
                        .losses
                        .iter()
                        .any(|loss| loss.code
                            == IgesLossCode::BoundaryPcurveOutsideSupportDomain.kind()),
                    "expected outside-support rejection for {count} controls: {:?}",
                    projection.losses
                );
                assert!(ir
                    .model
                    .faces
                    .iter()
                    .any(|face| face.id.as_str() == "iges:model:face#D9"));
                assert!(ir.model.pcurves.is_empty());
                assert!(ir
                    .model
                    .coedges
                    .iter()
                    .all(|coedge| coedge.pcurves.is_empty()));
                drop(projection);
                drop(derivations);
                drop(derivation_storage);
                drop(sequences);
                let released = ctx
                    .reserve_scoped(
                        policy.limits.max_materialized_bytes,
                        "test rejected alternate scratch released",
                    )
                    .unwrap();
                drop(released);
                let limit = match ctx.charge_retained(1, "test retained alternate output total") {
                    Err(CodecError::ResourceLimit(limit)) => limit,
                    Err(error) => return Err(error),
                    Ok(()) => return ctx.finish_session(),
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "test retained alternate output total");
                assert_eq!(limit.additional, 1);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == limit)
                );
                Err::<(), _>(CodecError::ResourceLimit(limit))
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected retained accounting refusal");
        };
        retained.push(limit.used);
    }
    assert_eq!(
        retained[0], retained[1],
        "rejected alternate size does not change retained output"
    );
}

#[test]
fn accepted_trimming_pcurves_promote_their_candidate_storage() {
    let bytes = multi_pcurve_boundary_file();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges trimming pcurve candidate storage",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            IgesCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        policy,
                        ..DecodeOptions::default()
                    },
                )
                .map(|_| ())
                .map_err(|error| match error {
                    cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
                    error => panic!("unexpected decode failure: {error:?}"),
                })
        },
    );
    // Each of the two degree-one pcurves has four knots and two 2D poles.
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "iges trimming pcurve candidate storage" && limit.additional == 128));
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().model.pcurves.is_empty());
    assert!(
        cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn rejected_trimmed_face_releases_previously_accepted_pcurve_buffers() {
    let bytes = subrange_nurbs_surface_boundary_file(2);
    let decoded = IgesCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let mut input = projection_input(&bytes);
    let mut collision = decoded
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == "iges:model:body#D9")
        .unwrap()
        .clone();
    collision.regions.clear();
    input.ir.model.bodies.push(collision);
    let corners = [
        Point3::new(0.2, 0.2, 0.0),
        Point3::new(0.8, 0.2, 0.0),
        Point3::new(0.8, 0.8, 0.0),
        Point3::new(0.2, 0.8, 0.0),
        Point3::new(0.2, 0.2, 0.0),
    ];
    let mut retained = Vec::new();
    for count in [5_usize, 513] {
        let points: Vec<_> = (0..count)
            .map(|index| {
                let scaled = 4 * index;
                let side = (scaled / (count - 1)).min(3);
                let t = f64::from(u32::try_from(scaled - side * (count - 1)).unwrap())
                    / f64::from(u32::try_from(count - 1).unwrap());
                let left = corners[side];
                let right = corners[side + 1];
                FinitePoint3::new(Point3::new(
                    left.x + t * (right.x - left.x),
                    left.y + t * (right.y - left.y),
                    0.0,
                ))
                .unwrap()
            })
            .collect();
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..count).map(|index| {
            f64::from(u32::try_from(index).unwrap()) / f64::from(u32::try_from(count - 1).unwrap())
        }));
        knots.push(1.0);
        let nurbs = crate::test_support::with_service_context(&[], |ctx| {
            NurbsCurve::new(ctx, 1, knots, NurbsPoles3::Polynomial { points }, false)
                .unwrap()
                .unwrap()
        });
        input
            .ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
            .unwrap()
            .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "iges trimming pcurve use traversal",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let mut ir = input.ir.clone();
                    let mut storage = ctx.reserve_scoped(0, "test rejected face derivations")?;
                    let mut sequences = super::super::super::geometry::SourceSequences::default();
                    super::super::project(
                        &mut ir,
                        &input.directory,
                        &input.records,
                        &input.global,
                        (ctx, &mut storage),
                        &mut sequences,
                    )
                    .map(|_| ())
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "iges trimming pcurve use traversal" && limit.additional == 1));
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "test rejected face retained output total",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = 1024 * 1024;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ir = input.ir.clone();
                let mut storage = ctx
                    .reserve_scoped(0, "test rejected face derivations")
                    .unwrap();
                let mut sequences = super::super::super::geometry::SourceSequences::default();
                let (projection, derivations) = super::super::project(
                    &mut ir,
                    &input.directory,
                    &input.records,
                    &input.global,
                    (&ctx, &mut storage),
                    &mut sequences,
                )
                .unwrap();
                assert!(!projection.decoded.contains(&9));
                assert_eq!(projection.losses.len(), 1);
                assert_eq!(projection.losses[0].message,
                "IGES entity type 144 form 0 was not projected: trimmed sheet candidate failed neutral validation");
                assert!(ir.model.faces.is_empty());
                assert!(ir.model.pcurves.is_empty());
                drop(projection);
                drop(derivations);
                drop(storage);
                drop(sequences);
                let released = ctx
                    .reserve_scoped(
                        policy.limits.max_materialized_bytes,
                        "test rejected face pcurve scratch released",
                    )
                    .unwrap();
                drop(released);
                let limit = match ctx.charge_retained(1, "test rejected face retained output total")
                {
                    Err(CodecError::ResourceLimit(limit)) => limit,
                    Err(error) => return Err(error),
                    Ok(()) => return ctx.finish_session(),
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "test rejected face retained output total");
                assert_eq!(limit.additional, 1);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == limit)
                );
                Err::<(), _>(CodecError::ResourceLimit(limit))
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected retained accounting refusal");
        };
        retained.push(limit.used);
    }
    assert_eq!(
        retained[0], retained[1],
        "a rejected face retains no candidate pcurve buffers"
    );
}
