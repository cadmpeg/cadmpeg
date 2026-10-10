// SPDX-License-Identifier: Apache-2.0

use crate::entities::geometry::SourceSequences;
use crate::IgesCodec;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;

#[test]
fn resolved_pcurve_vector_backing_is_scoped() {
    let source = CadIr::empty();
    let id = SurfaceId::mint("iges:model:surface#D1").unwrap();
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let support = super::super::SurfaceSupport {
        id: &id,
        geometry: &geometry,
        factor: 1.0,
    };
    let endpoints = super::super::PcurveEndpointCheck {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 0.0, 0.0),
        tolerance: 0.0,
    };
    let uses = [(false, 3), (false, 5)];
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges B-rep resolved pcurves",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut composite_storage = ctx.reserve_scoped(0, "test composite index")?;
            let mut model_index = None;
            let mut composite_index = None;
            let result = super::super::resolve_pcurve_uses(
                &source,
                &uses,
                &support,
                endpoints,
                &ctx,
                (
                    &mut model_index,
                    (&mut composite_index, &mut composite_storage),
                ),
            )
            .map(|_| ())
            .map_err(|error| match error {
                crate::entities::composite::CompositeCurveError::Budget(error) => error,
                error => panic!("unexpected resolution failure: {error:?}"),
            });
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "iges B-rep resolved pcurves"
            && limit.additional == u64::try_from(2 * std::mem::size_of::<(cadmpeg_ir::geometry::pcurve::PcurveGeometry, [f64; 2])>()).unwrap()));
}

#[test]
fn rejected_brep_drafts_release_payload_and_arena_storage() {
    let bytes = crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file_with_options(false, true);
    let decoded = IgesCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.id.as_str() != "iges:model:body#D55"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .ends_with("closed shell does not use every edge exactly twice with opposite senses")));
    let (mut directory, mut records, global) =
        crate::test_support::with_service_context(&bytes, |ctx| {
            let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
            let (global, _, _) = crate::global::parse(&scan, ctx).unwrap();
            let (directory, quarantined) =
                crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
            assert!(quarantined.is_empty());
            let records = crate::parameter::assemble_with_context(
                &scan,
                &directory,
                &quarantined,
                &global,
                ctx,
            )
            .unwrap()
            .records;
            (directory, records, global.length_context().unwrap())
        });
    let first_body = directory
        .iter()
        .find(|entry| entry.entity_type == 186)
        .unwrap()
        .clone();
    let first_record = records
        .iter()
        .find(|record| record.directory_sequence == first_body.sequence)
        .unwrap()
        .clone();
    // All bodies reach radial admission and reject after constructing complete drafts.
    for index in 1..16_u32 {
        let mut entry = first_body.clone();
        entry.sequence += 2 * index;
        let mut record = first_record.clone();
        record.directory_sequence = entry.sequence;
        directory.push(entry);
        records.push(record);
    }
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect();
    let parameters = records
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut ir = decoded.ir().clone();
    let before = ir.clone();
    let _probe = RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "iges B-rep identity copy",
        None,
    );
    let outcome = super::super::project(
        &mut ir,
        &directory,
        (&entries, &parameters),
        &global,
        &ctx,
        &mut sequences,
    )
    .unwrap();
    assert!(outcome.decoded.is_empty());
    assert_eq!(outcome.losses.len(), 16);
    assert!(outcome.losses.iter().all(|loss| loss
        .message
        .ends_with("closed shell does not use every edge exactly twice with opposite senses")));
    assert_eq!(ir, before);
    drop(outcome);
    drop(sequences);
    let free = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test rejected B-rep drafts released",
        )
        .unwrap();
    drop(free);
    ctx.finish_session().unwrap();
}
