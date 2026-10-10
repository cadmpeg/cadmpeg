// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn hem_reached_carrier_direction_reuses_the_actual_selected_carrier() {
    let (previous, transition, cylinders) = super::hem_source_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Diagnostic operation probe; old finite refusal controls stay unchanged.
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "find F3D Hem direction cylinder",
        None,
    );
    let direction = super::super::super::super::hem_direction_from_transition(
        &ctx,
        7,
        (&cylinders, &cylinders[0]),
        &transition.topology.surfaces.inserted,
        None,
        &previous,
        &transition,
    )
    .unwrap();
    assert_eq!(
        direction,
        Some(cadmpeg_ir::features::SheetMetalHemDirection::Forward)
    );
    assert_eq!(ctx.resource_refusal(), None);
    // The probe remains armed because the supplied carrier was not searched again.
    let Err(CodecError::ResourceLimit(original)) =
        ctx.charge_work(1, "find F3D Hem direction cylinder")
    else {
        panic!("the unused carrier lookup probe remains armed");
    };
    assert_eq!(original.operation, "find F3D Hem direction cylinder");
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(probe);
}

#[test]
fn hem_reached_carrier_direction_keeps_the_original_session_refusal() {
    let (previous, transition, cylinders) = super::hem_source_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
        policy.limits.max_work_units + 1,
        "original Hem carrier refusal",
    ) else {
        panic!("creating session refuses excess work");
    };
    assert!(
        matches!(super::super::super::super::hem_direction_from_transition(
        &ctx, 7, (&cylinders, &cylinders[0]), &transition.topology.surfaces.inserted,
        None, &previous, &transition,
    ), Err(CodecError::ResourceLimit(later)) if later == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn hem_reached_carrier_geometry_skips_inactive_and_nonparallel_native_prefixes() {
    let (scope, mut histories) = super::hem_geometry_fixture();
    let crate::history_records::AsmTopologyCache::Complete(topology) =
        &mut histories[0].states[0].topology_cache
    else {
        panic!("fixture has current native topology");
    };
    // A coaxial pair with radii 1.25 and 3.75 satisfies the Open gap rule.
    topology.surface_cylinders[1].origin = topology.surface_cylinders[0].origin;
    let mut inactive = topology.surface_cylinders[0].clone();
    inactive.surface = 99;
    inactive.origin.x = -1.0;
    let mut nonparallel = topology.surface_cylinders[0].clone();
    nonparallel.axis = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    nonparallel.origin.x = -1.0;
    topology.surface_cylinders.insert(0, nonparallel);
    topology.surface_cylinders.insert(0, inactive);
    let semantics = crate::test_support::with_decode_context(|ctx| {
        super::super::super::super::hem_geometry_semantics(ctx, &scope, 7, &histories)
    })
    .unwrap();
    assert_eq!(
        semantics.direction,
        Some(cadmpeg_ir::features::SheetMetalHemDirection::Forward)
    );
    assert_eq!(
        semantics.gap_length_form,
        Some(super::super::super::super::HemGapLengthForm::Open)
    );
}

#[test]
fn hem_reached_carrier_direction_preserves_distinct_eligible_plane_ambiguity() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalRelation,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let (mut previous, transition, cylinders) = super::hem_source_fixture();
    previous.face_loops = vec![
        AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        },
        AsmHistoricalRelation {
            owner_ref: 14,
            member_refs: vec![9],
        },
    ];
    previous.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 14,
        carrier: 21,
    });
    previous.surface_planes.push(AsmHistoricalPlane {
        surface: 21,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(1.0, 0.0, 0.0),
    });
    let direction = crate::test_support::with_decode_context(|ctx| {
        super::super::super::super::hem_direction_from_transition(
            ctx,
            7,
            (&cylinders, &cylinders[0]),
            &transition.topology.surfaces.inserted,
            None,
            &previous,
            &transition,
        )
    })
    .unwrap();
    // Both distinct planes prove the same sign. The native rule still refuses
    // to select a direction when a second eligible face has been reached.
    assert_eq!(direction, None);
    previous.surface_planes[1].normal = Vector3::new(0.0, 0.0, 0.0);
    let direction = crate::test_support::with_decode_context(|ctx| {
        super::super::super::super::hem_direction_from_transition(
            ctx,
            7,
            (&cylinders, &cylinders[0]),
            &transition.topology.surfaces.inserted,
            None,
            &previous,
            &transition,
        )
    })
    .unwrap();
    assert_eq!(
        direction,
        Some(cadmpeg_ir::features::SheetMetalHemDirection::Forward)
    );
}

#[test]
fn hem_reached_carrier_direction_does_not_repeat_the_cached_scale_scan() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalRelation,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    fn work(normal: Vector3) -> u64 {
        let (mut previous, transition, cylinders) = super::hem_source_fixture();
        previous.face_loops = vec![
            AsmHistoricalRelation {
                owner_ref: 4,
                member_refs: vec![5],
            },
            AsmHistoricalRelation {
                owner_ref: 14,
                member_refs: vec![9],
            },
        ];
        previous.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: 14,
            carrier: 21,
        });
        previous.surface_planes.push(AsmHistoricalPlane {
            surface: 21,
            origin: Point3::new(1.0, 0.0, 0.0),
            normal,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let direction = super::super::super::super::hem_direction_from_transition(
            &ctx,
            7,
            (&cylinders, &cylinders[0]),
            &transition.topology.surfaces.inserted,
            None,
            &previous,
            &transition,
        )
        .unwrap();
        assert_eq!(
            direction,
            Some(cadmpeg_ir::features::SheetMetalHemDirection::Forward)
        );
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            policy.limits.max_work_units + 1,
            "measure completed Hem scale work",
        ) else {
            panic!("finite work control exposes cumulative usage");
        };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        usage.used
    }
    // Both runs reach the same two distinct face/plane lookups. A zero normal
    // stops before scale use. The unit normal reaches the cached scale, then
    // the first cylinder's zero plane offset stops before carrier offsets.
    // Neither route executes another input-sized scan.
    assert_eq!(
        work(Vector3::new(1.0, 0.0, 0.0)),
        work(Vector3::new(0.0, 0.0, 0.0))
    );
}
