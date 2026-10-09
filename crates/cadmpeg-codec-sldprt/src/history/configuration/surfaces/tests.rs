// SPDX-License-Identifier: Apache-2.0

use super::configuration_surface_carriers;
use crate::history::tests::design_configuration;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignConfiguration, DistinctMembers};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Face, FaceLoops, Region, Sense, Shell};

fn fixture() -> cadmpeg_ir::CadIr {
    let body = BodyId::mint("synthetic:test:id#body").unwrap();
    let region = RegionId::mint("synthetic:test:id#region").unwrap();
    let shell = ShellId::mint("synthetic:test:id#shell").unwrap();
    let face = FaceId::mint("synthetic:test:id#face").unwrap();
    let surface = SurfaceId::mint("synthetic:test:id#surface").unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(Shell::new(
        shell.clone(), region, vec![face.clone()], Vec::new(), Vec::new(),
    ).unwrap());
    ir.model.faces.push(Face {
        id: face,
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: FaceLoops::Unspecified { loops: Vec::new() },
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ).unwrap(),
        )),
        source_object: None,
    });
    let mut configuration = design_configuration("ancestry", 0, Some(0), None);
    configuration.bodies = Some(DistinctMembers::try_from(
        vec![body], &cadmpeg_test_support::service_decode_context(),
    ).unwrap());
    ir.model.configurations.push(configuration);
    ir
}

fn first_set_storage<T>() -> u64 {
    // Core's first set storage bound: four buckets, their control bytes,
    // sixteen trailing controls and max(alignment, 16)-1 padding bytes.
    u64::try_from(4 * std::mem::size_of::<T>() + 4 + 16 + std::mem::align_of::<T>().max(16) - 1)
        .unwrap()
}

fn traversal_peak() -> u64 {
    let stages = [
        first_set_storage::<&BodyId>(),
        first_set_storage::<&RegionId>(),
        first_set_storage::<&ShellId>(),
        first_set_storage::<&FaceId>(),
        first_set_storage::<&SurfaceId>(),
    ];
    stages.windows(2).map(|pair| pair[0] + pair[1]).max().unwrap()
}

fn output_storage(ir: &cadmpeg_ir::CadIr) -> u64 {
    // Plane geometry and absent source metadata have no dynamic children.
    // Surface copies retain one exact identity and the first Vec growth slots.
    let width = std::mem::size_of::<Surface>();
    let slots = match width { 0 => 0, 1 => 8, 2..=1024 => 4, _ => 1 };
    u64::try_from(ir.model.surfaces[0].id.as_str().len() + slots * width).unwrap()
}

#[test]
fn configuration_surface_ancestry_keeps_only_adjacent_stage_sets() {
    let ir = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = traversal_peak();
    policy.limits.max_retained_bytes = output_storage(&ir);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(configuration_surface_carriers(&ctx, &ir, 0).unwrap(), ir.model.surfaces);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_surface_ancestry_expires_before_scoped_output_growth() {
    let ir = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = traversal_peak().max(
        first_set_storage::<&SurfaceId>() + output_storage(&ir),
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output_storage = ctx.reserve_scoped(0, "SLDPRT configuration surface carriers").unwrap();
    let output = output_storage.with_storage(|| configuration_surface_carriers(&ctx, &ir, 0)).unwrap();
    assert_eq!(output, ir.model.surfaces);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn configuration_surface_ancestry_refusal_preserves_the_original_fuse() {
    let ir = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = traversal_peak() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = configuration_surface_carriers(&ctx, &ir, 0).unwrap_err() else {
        panic!("expected ancestry storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT configuration surface ancestry");
    assert_eq!(limit.limit, traversal_peak() - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(replayed) = configuration_surface_carriers(&ctx, &ir, 0).unwrap_err() else {
        panic!("expected original ancestry refusal");
    };
    assert_eq!(replayed, limit);
}

#[test]
fn configuration_surface_output_keeps_its_retained_refusal_boundary() {
    let ir = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = traversal_peak();
    policy.limits.max_retained_bytes = output_storage(&ir) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = configuration_surface_carriers(&ctx, &ir, 0).unwrap_err() else {
        panic!("expected surface output storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "copy SLDPRT configuration surface carriers");
    assert_eq!(limit.limit, output_storage(&ir) - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(replayed) = configuration_surface_carriers(&ctx, &ir, 0).unwrap_err() else {
        panic!("expected original output refusal");
    };
    assert_eq!(replayed, limit);
}

#[test]
fn unresolved_configuration_body_membership_reuses_model_surface_carriers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#model-surface")
            .expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        bodies: None,
        ..design_configuration("unresolved", 0, Some(0), None)
    });

    assert_eq!(
        configuration_surface_carriers(&cadmpeg_test_support::service_decode_context(), &ir, 0)
            .unwrap(),
        ir.model.surfaces,
    );
}

#[test]
fn resolved_empty_configuration_body_membership_has_no_surface_carriers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#model-surface")
            .expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model
        .configurations
        .push(design_configuration("empty", 0, Some(0), None));

    assert!(configuration_surface_carriers(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        0
    )
    .unwrap()
    .is_empty());
}

