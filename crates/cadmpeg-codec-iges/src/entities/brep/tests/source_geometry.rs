// SPDX-License-Identifier: Apache-2.0

use crate::entities::geometry::SourceSequences;
use crate::IgesCodec;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
use cadmpeg_ir::ids::ProceduralSurfaceId;

fn shared_brep_source_geometry_work(body_count: u32, construction_count: u32) -> u64 {
    let bytes = crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file();
    let decoded = IgesCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let (mut directory, mut records, global) =
        crate::test_support::with_service_context(&bytes, |ctx| {
            let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
            let (global, _, _storage) = crate::global::parse(&scan, ctx).unwrap();
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
    let original = *directory
        .iter()
        .find(|entry| entry.entity_type == 514)
        .unwrap();
    let original_record = records
        .iter()
        .find(|record| record.directory_sequence == original.sequence)
        .unwrap()
        .clone();
    for index in 1..body_count {
        let mut entry = original;
        entry.sequence += 2 * index;
        let mut record = original_record.clone();
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
    let mut source = decoded.ir().clone();
    source.model.bodies.clear();
    source.model.regions.clear();
    source.model.shells.clear();
    source.model.faces.clear();
    source.model.loops.clear();
    source.model.coedges.clear();
    source.model.pcurves.clear();
    source
        .model
        .edges
        .retain(|edge| !edge.id.as_str().contains("#D27:"));
    source
        .model
        .vertices
        .retain(|vertex| !vertex.id.as_str().contains("#D27:"));
    source
        .model
        .points
        .retain(|point| !point.id.as_str().contains("#D27:"));
    for index in 0..construction_count {
        source
            .model
            .procedural_surfaces
            .push(ProceduralSurface::new(
                ProceduralSurfaceId::mint(format!("test:model:construction#unused{index}"))
                    .unwrap(),
                ProceduralSurfaceDefinition::Unknown {
                    record: None,
                    cache: None,
                },
                None,
            ));
    }
    let run = |ctx: &DecodeContext<'_>| -> Result<(), CodecError> {
        let mut ir = source.clone();
        let mut sequences = SourceSequences::new(ctx)?;
        let outcome = super::super::project(
            &mut ir,
            &directory,
            (&entries, &parameters),
            &global,
            ctx,
            &mut sequences,
        )?;
        assert!(outcome.losses.is_empty(), "{:?}", outcome.losses);
        assert_eq!(ir.model.bodies.len(), usize::try_from(body_count).unwrap());
        for index in 0..body_count {
            assert!(outcome.decoded.contains(&(original.sequence + 2 * index)));
        }
        Ok(())
    };
    crate::test_support::with_service_context(&[], |ctx| run(ctx).unwrap());
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "test shared B-rep geometry work",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                run(ctx)?;
                ctx.charge_work(1, "test shared B-rep geometry work")
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected work accounting refusal")
    };
    assert_eq!(limit.operation, "test shared B-rep geometry work");
    assert_eq!(limit.additional, 1);
    limit.used
}

#[test]
fn shared_brep_source_geometry_work_grows_below_quadratic() {
    let small = shared_brep_source_geometry_work(64, 1024);
    let large = shared_brep_source_geometry_work(128, 2048);
    assert!(
        large < 5 * small / 2,
        "doubling source geometry and bodies: {small} -> {large}"
    );
}
