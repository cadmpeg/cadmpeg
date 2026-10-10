// SPDX-License-Identifier: Apache-2.0
use super::{model_jet, HigherPartials, SurfaceRequest};
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{EvaluationFailure, SurfaceJet};
use crate::geometry::analytic::{CylinderSurface, PlaneSurface};
use crate::geometry::surface_payloads::OffsetSurfaceConstruction;
use crate::geometry::{LegacyExtensionFlags, OffsetExtension, ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use crate::ids::{ProceduralSurfaceId, SurfaceId};
use crate::index::{ModelIndex, StandardIndex};
use crate::math::{Point3, Vector3};
use crate::transform::Transform;
use crate::CadIr;

mod nurbs;
mod analytic;
mod procedural_third;
mod procedural_fourth;
mod orientation;
mod fourth;
mod fifth;

fn cylinder() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0,
    ).unwrap())
}

fn plane() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
    ).unwrap())
}

fn stored(ir: &mut CadIr, name: &str, geometry: SolvedSurfaceGeometry) -> SurfaceId {
    let id = SurfaceId::mint(format!("test:model:requested-surface#{name}")).unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(), geometry: SurfaceGeometry::Solved(geometry), source_object: None });
    id
}

fn procedural(ir: &mut CadIr, name: &str, definition: ProceduralSurfaceDefinition) -> SurfaceId {
    let id = SurfaceId::mint(format!("test:model:requested-surface#{name}")).unwrap();
    let construction = ProceduralSurfaceId::mint(format!("test:model:requested-construction#{name}")).unwrap();
    ir.model.surfaces.push(Surface {
        id: id.clone(), geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }), source_object: None,
    });
    ir.model.add_procedural_surface(&crate::document::admission::StandardAdmission, &id,
        ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
    id
}

fn offset(ir: &mut CadIr, name: &str, support: SurfaceId, distance: f64) -> SurfaceId {
    procedural(ir, name, ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        support, distance, None, None, false,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None },
    ).unwrap()))
}

fn evaluate(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64, request: SurfaceRequest) -> SurfaceJet {
    let index = ModelIndex::build(ir, StandardIndex);
    model_jet(EvaluationAdmission::Standard, &index, surface, u, v, request).unwrap()
}

#[test]
fn requested_offset_cylinder_second_follows_radius_plus_distance() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cylinder", cylinder());
    let shifted = offset(&mut ir, "offset", base, 1.0);
    let result = evaluate(&ir, &shifted, 0.0, 0.25, SurfaceRequest::Second).second_partials().unwrap().into_raw();
    // O(u,v)=(3 cos u,3 sin u,v).
    assert_eq!(result.point, Point3::new(3.0, 0.0, 0.25));
    assert_eq!(result.du, Vector3::new(0.0, 3.0, 0.0));
    assert_eq!(result.duu, Vector3::new(-3.0, 0.0, 0.0));
    assert_eq!(result.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(result.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn requested_offset_replica_segments_keep_placement_order_and_missing_fourth() {
    let scale = Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap();
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cylinder", cylinder());
    let inner = offset(&mut ir, "inner", base.clone(), 1.0);
    let placed_inner = procedural(&mut ir, "placed-inner", ProceduralSurfaceDefinition::Replica { source: inner, transform: scale });
    assert_eq!(evaluate(&ir, &placed_inner, 0.0, 0.0, SurfaceRequest::Second).second.unwrap()[0].get(), Vector3::new(-6.0, 0.0, 0.0));
    let placed_base = procedural(&mut ir, "placed-base", ProceduralSurfaceDefinition::Replica { source: base, transform: scale });
    let outer = offset(&mut ir, "outer", placed_base, 1.0);
    let result = evaluate(&ir, &outer, 0.0, 0.0, SurfaceRequest::Second);
    // Ellipse a=4,b=2: n_u=(0,a/b,0), n_uu=(-a*a/(b*b),0,0) at u=0.
    assert_eq!(result.point.get(), Point3::new(5.0, 0.0, 0.0));
    assert_eq!(result.first.unwrap()[0].get(), Vector3::new(0.0, 4.0, 0.0));
    assert_eq!(result.second.unwrap()[0].get(), Vector3::new(-8.0, 0.0, 0.0));
    let missing = offset(&mut ir, "needs-fourth", placed_inner, 1.0);
    let result = evaluate(&ir, &missing, 0.0, 0.0, SurfaceRequest::Second);
    assert_eq!(result.point.get(), Point3::new(7.0, 0.0, 0.0));
    assert!(result.first.is_ok());
    // The placed inner chart is the ellipse a=6,b=3; its unit offset
    // has uu=-a-a*a/(b*b)=-10 at zero.
    assert_eq!(result.second.unwrap()[0].get(), Vector3::new(-10.0, 0.0, 0.0));
}

#[test]
fn requested_affine_chart_preserves_plane_orders_through_nested_offsets() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "plane", plane());
    let inner = offset(&mut ir, "inner", base, 1.0);
    let placed = procedural(&mut ir, "placed", ProceduralSurfaceDefinition::Replica {
        source: inner, transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0]]).unwrap(),
    });
    let outer = offset(&mut ir, "outer", placed, 2.0);
    let result = evaluate(&ir, &outer, 0.25, 0.5, SurfaceRequest::Second).second_partials().unwrap().into_raw();
    assert_eq!(result.point, Point3::new(0.5, 1.5, 6.0));
    assert_eq!(result.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(result.dv, Vector3::new(0.0, 3.0, 0.0));
    assert_eq!([result.duu, result.duv, result.dvv], [Vector3::new(0.0, 0.0, 0.0); 3]);
}

#[test]
fn requested_subset_offsets_keep_orientation_and_derivative_signs() {
    use crate::geometry::surface_payloads::SubsetSurfaceConstruction;
    for reversed in [[true, false], [false, true], [true, true]] {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, "cylinder", cylinder());
        let subset = procedural(&mut ir, "subset", ProceduralSurfaceDefinition::Subset(
            SubsetSurfaceConstruction::try_new(base, [[0.0, 1.0], [0.0, 1.0]], Some(!reversed[0]), Some(!reversed[1]), None).unwrap(),
        ));
        let shifted = offset(&mut ir, "offset", subset, 1.0);
        let result = evaluate(&ir, &shifted, 0.0, 0.0, SurfaceRequest::Second).second_partials().unwrap().into_raw();
        let radius = if reversed[0] == reversed[1] { 3.0 } else { 1.0 };
        assert_eq!(result.point, Point3::new(radius, 0.0, 0.0));
        assert_eq!(result.du, Vector3::new(0.0, if reversed[0] { -radius } else { radius }, 0.0));
        assert_eq!(result.dv, Vector3::new(0.0, 0.0, if reversed[1] { -1.0 } else { 1.0 }));
        assert_eq!(result.duu, Vector3::new(-radius, 0.0, 0.0));
    }
}

#[test]
fn requested_zero_and_cancelled_offset_preserve_exact_stored_orders() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cylinder", cylinder());
    let zero = offset(&mut ir, "zero", base.clone(), 0.0);
    let first = offset(&mut ir, "first", base.clone(), 1.0);
    let cancelled = offset(&mut ir, "cancelled", first, -1.0);
    for id in [zero, cancelled] {
        let expected = evaluate(&ir, &base, 0.3, 0.7, SurfaceRequest::Second).second_partials();
        assert_eq!(evaluate(&ir, &id, 0.3, 0.7, SurfaceRequest::Second).second_partials(), expected);
    }
}

#[test]
fn requested_placement_recipe_admits_each_actual_source_node_before_following_it() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use super::{Mapping, Source};
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "plane", plane());
    let index = ModelIndex::build(&ir, StandardIndex);
    let geometry = &ir.model.surfaces[0].geometry;
    let first = Mapping {
        source: Source::Stored(geometry, 0.25, 0.5), distance: 0.0, reversed: [false, false], orientation: 1.0,
    };
    let second = Mapping {
        source: Source::Replica(&first, Transform::identity()), distance: 0.0, reversed: [false, false], orientation: 1.0,
    };
    let third = Mapping {
        source: Source::Replica(&second, Transform::identity()), distance: 0.0, reversed: [false, false], orientation: 1.0,
    };
    assert_eq!(ir.model.surfaces[0].id, base);
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = third.evaluate(EvaluationAdmission::Decode(&ctx), &index, SurfaceRequest::Second);
        if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else {
                panic!("the next actual recipe source node must refuse");
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert_eq!(original.operation, "IR surface placement source traversal");
            assert!(matches!(third.evaluate(EvaluationAdmission::Decode(&ctx), &index, SurfaceRequest::First), Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        } else {
            let result = result.unwrap();
            assert_eq!(result.jet.point.get(), Point3::new(0.25, 0.5, 0.0));
            assert_eq!(result.jet.second.unwrap(), [crate::features::FiniteVector3::ZERO; 3]);
            ctx.finish_session().unwrap();
        }
    }
}
