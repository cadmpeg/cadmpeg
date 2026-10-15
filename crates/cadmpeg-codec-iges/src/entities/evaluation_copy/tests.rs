// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodePolicy, ResourceDimension,
};
use cadmpeg_ir::geometry::{CompoundComponent, CompoundCurveConstruction};
use cadmpeg_ir::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

fn child_refusal(
    used: u64,
    additional: u64,
    run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        COPY,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let _probe = (cap == u64::MAX)
                .then(|| RefusalProbe::arm(ResourceDimension::WorkUnits, COPY, Some(additional)));
            let result = run(&ctx);
            if let Err(CodecError::ResourceLimit(first)) = &result {
                assert_eq!(first.operation, COPY);
                assert_eq!((first.used, first.additional), (used, additional));
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first)
                );
            }
            result
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.used == used && limit.additional == additional)
    );
}

fn key(kind: &str, position: usize, target: usize) -> String {
    format!(
        "test:model:{kind}#{}",
        if position == target {
            "x".repeat(101)
        } else {
            position.to_string()
        }
    )
}

fn boundaries(pcurves: bool) {
    const COUNT: usize = 64;
    for target in [0, COUNT - 1] {
        let support = SurfaceId::mint("test:model:surface#support").unwrap();
        let keys: Vec<_> = (0..COUNT)
            .map(|position| key(if pcurves { "pcurve" } else { "curve" }, position, target))
            .collect();
        let expected_work = u64_from_index(
            support.as_str().len()
                + COUNT
                + target
                + 1
                + keys[..target].iter().map(String::len).sum::<usize>(),
        );
        let source = ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#bounded").unwrap(),
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries: if pcurves {
                    Vec::new()
                } else {
                    keys.iter()
                        .map(|key| CurveId::mint(key.clone()).unwrap())
                        .collect()
                },
                boundary_pcurves: if pcurves {
                    keys.iter()
                        .map(|key| PcurveId::mint(key.clone()).unwrap())
                        .collect()
                } else {
                    Vec::new()
                },
                implicit_outer: false,
            },
            None,
        );
        let before = source.clone();
        child_refusal(expected_work, u64_from_index(keys[target].len()), |ctx| {
            let result = procedural_surface(&source, ctx).map(|_| ());
            assert_eq!(source, before);
            result
        });
    }
}

#[test]
fn evaluation_boundary_identity_copy_refuses_first_and_last_without_visiting_the_tail() {
    boundaries(false);
}

#[test]
fn evaluation_boundary_pcurve_identity_copy_refuses_first_and_last_without_visiting_the_tail() {
    boundaries(true);
}

#[test]
fn evaluation_compound_identity_copy_refuses_first_and_last_without_visiting_the_tail() {
    const COUNT: usize = 64;
    for target in [0, COUNT - 1] {
        let keys: Vec<_> = (0..COUNT)
            .map(|position| key("curve", position, target))
            .collect();
        let components = keys
            .iter()
            .map(|key| CompoundComponent {
                parameter: 0.0,
                component: CurveId::mint(key.clone()).unwrap(),
            })
            .collect();
        let source = ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:procedural-curve#compound").unwrap(),
            ProceduralCurveDefinition::Compound(
                CompoundCurveConstruction::try_new(Vec::new(), components, None).unwrap(),
            ),
        );
        let before = source.clone();
        let expected_work = u64_from_index(
            COUNT + target + 1 + keys[..target].iter().map(String::len).sum::<usize>(),
        );
        child_refusal(expected_work, u64_from_index(keys[target].len()), |ctx| {
            let result = procedural_curve(&source, ctx).map(|_| ());
            assert_eq!(source, before);
            result
        });
    }
}
