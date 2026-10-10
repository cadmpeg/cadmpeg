// SPDX-License-Identifier: Apache-2.0

use crate::brep::AsmBrep;
use crate::nurbs::proc_surface::{
    ClassicLoftProfileData, EmbeddedCompoundLoft, EmbeddedCompoundLoftDirection,
    EmbeddedCompoundLoftScale, EmbeddedCompoundLoftTail, EmbeddedLoftPathLayout,
    EmbeddedRevisionLoftPathCurve, EmbeddedScaledCompoundLoft, EmbeddedScaledCompoundLoftBranch,
    EmbeddedScaledCompoundLoftShape, LoftProfileData,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{
    LoftMemberForm, LoftSubdata, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use std::cell::Cell;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap()
    .unwrap()
}

fn support(classic: bool) -> LoftProfileData {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let subdata = LoftSubdata::Type211 {
        dimensions: [0, 0],
        row: [0.0, 1.0],
    };
    if classic {
        LoftProfileData::Classic(ClassicLoftProfileData {
            type_code: 1,
            surface,
            pcurve: None,
            first_flag: false,
            asm_extension: 0,
            subdata,
            direction: None,
        })
    } else {
        LoftProfileData::RevisionSupport {
            endpoints: [Some(0.0), Some(1.0)],
            type_code: std::num::NonZeroI64::new(1).unwrap(),
            surface: Some(surface),
            support_bounds: [None; 4],
            pcurve: None,
            first_flag: false,
            asm_extension: None,
            subdata,
            direction: None,
        }
    }
}

fn present_path(revision: bool) {
    let geometry = curve();
    let expected = geometry.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let calls = Cell::new(0);
    let id = CurveId::mint("sat:brep:procedural_surface#7:loft:0:0:path").unwrap();
    let expected_id = id.clone();
    let layout = if revision {
        EmbeddedLoftPathLayout::Revision(Some(EmbeddedRevisionLoftPathCurve {
            geometry,
            endpoints: [Some(0.0), Some(1.0)],
        }))
    } else {
        EmbeddedLoftPathLayout::Legacy(geometry)
    };
    let mut out = AsmBrep::default();
    let path = super::super::emit_loft_path_curve(&ctx, &mut out, layout, || {
        calls.set(calls.get() + 1);
        Ok(id)
    })
    .unwrap()
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(path.id, expected_id);
    assert_eq!(path.endpoints, revision.then_some([Some(0.0), Some(1.0)]));
    assert_eq!(out.curves.len(), 1);
    assert_eq!(out.curves[0].id, expected_id);
    assert!(matches!(&out.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(actual)) if actual == &expected));
    ctx.finish_session().unwrap();
}

#[test]
fn legacy_loft_path_calls_identity_once_and_keeps_geometry() {
    present_path(false);
}

#[test]
fn revision_loft_path_calls_identity_once_and_keeps_endpoints() {
    present_path(true);
}

fn present_support(classic: bool) {
    let data = support(classic);
    let expected_geometry = match &data {
        LoftProfileData::Classic(data) => data.surface.clone(),
        LoftProfileData::RevisionSupport {
            surface: Some(surface),
            ..
        } => surface.clone(),
        _ => panic!("present support fixture"),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let id = SurfaceId::mint("sat:brep:procedural_surface#7:loft:0:0:support:0").unwrap();
    let expected_id = id.clone();
    let calls = Cell::new(0);
    let mut out = AsmBrep::default();
    let form = super::super::emit_loft_member_form(&ctx, &mut out, data, || {
        calls.set(calls.get() + 1);
        Ok(id)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(
        matches!(form, LoftMemberForm::Support { type_code: 1, surface: Some(surface),
        support_bounds: [None, None, None, None], pcurve: None, first_flag: false,
        asm_extension, subdata: LoftSubdata::Type211 { dimensions: [0, 0], row },
        direction: None } if surface == expected_id && asm_extension == classic.then_some(0)
            && row == [0.0, 1.0])
    );
    assert_eq!(out.surfaces.len(), 1);
    assert_eq!(out.surfaces[0].id, expected_id);
    assert_eq!(out.surfaces[0].geometry, expected_geometry);
    ctx.finish_session().unwrap();
}

#[test]
fn classic_loft_support_calls_identity_once_and_keeps_geometry() {
    present_support(true);
}

#[test]
fn revision_loft_support_calls_identity_once_and_keeps_geometry() {
    present_support(false);
}

#[test]
fn present_loft_members_skip_identity_after_original_refusal() {
    let geometry = curve();
    crate::test_support::with_entry_context(|ctx, original| {
        let Some(first) = original else {
            return;
        };
        let mut out = AsmBrep::default();
        for layout in [
            EmbeddedLoftPathLayout::Legacy(geometry.clone()),
            EmbeddedLoftPathLayout::Revision(Some(EmbeddedRevisionLoftPathCurve {
                geometry: geometry.clone(),
                endpoints: [Some(0.0), Some(1.0)],
            })),
        ] {
            assert!(
                matches!(super::super::emit_loft_path_curve(ctx, &mut out, layout,
                || panic!("refused present path skips identity")),
                Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first)
            );
        }
        for classic in [true, false] {
            assert!(
                matches!(super::super::emit_loft_member_form(ctx, &mut out, support(classic),
                || panic!("refused present support skips identity")),
                Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first)
            );
        }
        assert!(out.curves.is_empty() && out.surfaces.is_empty());
    });
}

#[test]
fn loft_identity_factory_refusal_precedes_output_mutation() {
    let geometry = curve();
    for scenario in 0..4 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = AsmBrep::default();
        let error = if scenario < 2 {
            let layout = if scenario == 0 {
                EmbeddedLoftPathLayout::Legacy(geometry.clone())
            } else {
                EmbeddedLoftPathLayout::Revision(Some(EmbeddedRevisionLoftPathCurve {
                    geometry: geometry.clone(),
                    endpoints: [None; 2],
                }))
            };
            super::super::emit_loft_path_curve(&ctx, &mut out, layout, || {
                ctx.charge_work(1, "test original identity factory refusal")?;
                panic!("factory refuses before identity exists")
            })
            .unwrap_err()
        } else {
            super::super::emit_loft_member_form(&ctx, &mut out, support(scenario == 2), || {
                ctx.charge_work(1, "test original identity factory refusal")?;
                panic!("factory refuses before identity exists")
            })
            .unwrap_err()
        };
        let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
            panic!("original refusal");
        };
        assert_eq!(
            first.dimension,
            cadmpeg_core::decode::ResourceDimension::WorkUnits
        );
        assert_eq!(first.operation, "test original identity factory refusal");
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
        assert!(out.curves.is_empty() && out.surfaces.is_empty());
        for _ in 0..64 {
            assert!(matches!(super::super::emit_loft_path_curve(&ctx, &mut out,
                EmbeddedLoftPathLayout::Revision(None), || panic!("fused factory cannot run")),
                Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first));
            assert!(
                matches!(super::super::emit_loft_member_form(&ctx, &mut out, support(true),
                || panic!("fused factory cannot run")),
                Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first)
            );
        }
        assert!(matches!(ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first));
    }
}

fn scale() -> EmbeddedCompoundLoftScale {
    EmbeddedCompoundLoftScale {
        members: Vec::new(),
        path: curve(),
        auxiliaries: Vec::new(),
        tail: [0, 0],
    }
}

fn compound_scales(hole: bool) {
    let slots = if hole {
        [None, None, Some(scale()), None]
    } else {
        [Some(scale()), Some(scale()), Some(scale()), None]
    };
    let embedded = EmbeddedCompoundLoft {
        scales: Box::new(slots),
        fifth_scale: None,
        flags: [false; 2],
        tail: EmbeddedCompoundLoftTail::Zero {
            flags: [false; 2],
            direction: EmbeddedCompoundLoftDirection::Vector(Vector3::new(0.0, 0.0, 1.0)),
            trailing_flags: [false; 2],
        },
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut out = AsmBrep::default();
    let definition = super::super::emit_compound_loft_surface(
        &ctx,
        &mut out,
        7,
        embedded,
        crate::asm_format!("sat"),
    );
    if hole {
        assert!(
            matches!(definition, Err(cadmpeg_core::CodecError::Malformed(message))
            if message == "compound loft scales must form a leading prefix")
        );
        assert_eq!(out.curves.len(), 1);
        assert_eq!(
            out.curves[0].id.as_str(),
            "sat:brep:procedural_surface#7:cloft:scale2:path"
        );
        ctx.finish_session().unwrap();
        return;
    }
    let ProceduralSurfaceDefinition::CompoundLoft(payload) = definition.unwrap() else {
        panic!("compound loft");
    };
    let scales = payload.construction().scales.as_slice();
    assert_eq!(scales.len(), 3);
    for (index, scale) in scales.iter().enumerate() {
        assert_eq!(
            scale.path.as_str(),
            format!("sat:brep:procedural_surface#7:cloft:scale{index}:path")
        );
        assert_eq!(out.curves[index].id, scale.path);
    }
    assert_eq!(out.curves.len(), 3);
    ctx.finish_session().unwrap();
}

fn scaled_compound_scales(hole: bool) {
    let slots = if hole {
        [None, None, Some(scale())]
    } else {
        [Some(scale()), Some(scale()), Some(scale())]
    };
    let embedded = Box::new(EmbeddedScaledCompoundLoft {
        singularity: 0,
        shape: EmbeddedScaledCompoundLoftShape::Full,
        discontinuities: std::array::from_fn(|_| Vec::new()),
        discontinuity_flag: false,
        scales: Box::new(slots),
        flags: [false; 2],
        selector: 0,
        branch: EmbeddedScaledCompoundLoftBranch::Direct {
            flag: false,
            direction: EmbeddedCompoundLoftDirection::Vector(Vector3::new(0.0, 0.0, 1.0)),
        },
        trailing_flags: [false; 2],
        tail_kind: 0,
        tail_directions: [Vector3::new(0.0, 0.0, 1.0); 2],
        tail_singularity: 0,
        tail_curve: curve(),
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut out = AsmBrep::default();
    let definition = super::super::emit_scaled_compound_loft_surface(
        &ctx,
        &mut out,
        7,
        embedded,
        crate::asm_format!("sat"),
    );
    if hole {
        assert!(
            matches!(definition, Err(cadmpeg_core::CodecError::Malformed(message))
            if message == "compound loft scales must form a leading prefix")
        );
        assert_eq!(out.curves.len(), 2);
        assert_eq!(
            out.curves[0].id.as_str(),
            "sat:brep:procedural_surface#7:scaled_cloft:scale2:path"
        );
        assert_eq!(
            out.curves[1].id.as_str(),
            "sat:brep:procedural_surface#7:scaled_cloft:tail:curve"
        );
        ctx.finish_session().unwrap();
        return;
    }
    let ProceduralSurfaceDefinition::ScaledCompoundLoft(payload) = definition.unwrap() else {
        panic!("scaled compound loft");
    };
    let scales = payload.construction().scales.as_slice();
    assert_eq!(scales.len(), 3);
    for (index, scale) in scales.iter().enumerate() {
        assert_eq!(
            scale.path.as_str(),
            format!("sat:brep:procedural_surface#7:scaled_cloft:scale{index}:path")
        );
        assert_eq!(out.curves[index].id, scale.path);
    }
    assert_eq!(out.curves.len(), 4);
    assert_eq!(
        out.curves[3].id.as_str(),
        "sat:brep:procedural_surface#7:scaled_cloft:tail:curve"
    );
    ctx.finish_session().unwrap();
}

#[test]
fn compound_loft_keeps_leading_scale_ordinals_with_absent_tail() {
    compound_scales(false);
}

#[test]
fn compound_loft_keeps_nonprefix_scale_rejection() {
    compound_scales(true);
}

#[test]
fn scaled_compound_loft_keeps_leading_scale_ordinals() {
    scaled_compound_scales(false);
}

#[test]
fn scaled_compound_loft_keeps_nonprefix_scale_rejection() {
    scaled_compound_scales(true);
}

#[test]
fn law_curve_missing_domain_executes_no_identity_or_output_work() {
    use crate::nurbs::proc_curve::{EmbeddedLawCurve, EmbeddedLawCurveLayout, SupportSlot};
    use crate::nurbs::proc_surface::EmbeddedLawFormula;
    let embedded = EmbeddedLawCurve {
        surfaces: [SupportSlot::Absent, SupportSlot::Absent],
        pcurves: [None, None],
        discontinuities: std::array::from_fn(|_| Vec::new()),
        layout: EmbeddedLawCurveLayout::Version {
            stamp: 23_100,
            post_enum: 0,
            parameter_range: [None; 2],
        },
        extension: 0,
        primary: EmbeddedLawFormula::Null,
        additional: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    assert!(matches!(
        super::super::emit_law_curve(&ctx, &mut out, 7, embedded, crate::asm_format!("sat"), None),
        Err(super::super::CarrierCurveError::Invalid(
            "missing procedural curve cache domain"
        ))
    ));
    assert!(out.curves.is_empty() && out.surfaces.is_empty());
    ctx.finish_session().unwrap();
}

fn revision_profiles(present: bool) {
    use crate::nurbs::proc_surface::{
        EmbeddedLoftPath, EmbeddedLoftProfileMember, EmbeddedLoftSectionEntry,
        EmbeddedRevisionCompoundLoft,
    };
    use cadmpeg_ir::geometry::{RevisionCacheForm, RevisionCompoundLoftTail};
    let profile = || {
        if present {
            vec![EmbeddedLoftProfileMember {
                curve: curve(),
                data: LoftProfileData::RevisionPcurvePair {
                    endpoints: [Some(0.0), Some(1.0)],
                    pcurve: None,
                    secondary_pcurve: None,
                    asm_extension: None,
                    subdata: LoftSubdata::Type211 {
                        dimensions: [0, 0],
                        row: [0.0, 1.0],
                    },
                    direction: None,
                },
            }]
        } else {
            Vec::new()
        }
    };
    let path = || EmbeddedLoftPath {
        layout: EmbeddedLoftPathLayout::Revision(None),
        auxiliaries: Vec::new(),
        flag: 0,
    };
    let embedded = Box::new(EmbeddedRevisionCompoundLoft {
        revision: cadmpeg_ir::scalar::PositiveI64::new(23_100).unwrap(),
        cache: RevisionCacheForm::SolvedCache {
            fit_tolerance: cadmpeg_ir::geometry::FitTolerance::try_new(0.0).unwrap(),
        },
        discontinuities: std::array::from_fn(|_| Vec::new()),
        tail_flag: false,
        base_profile: profile(),
        base_path: path(),
        entries: vec![EmbeddedLoftSectionEntry {
            parameter: 1.0,
            profile: profile(),
            path: path(),
        }],
        flags: [false; 2],
        kind_flags: [false; 2],
        direction: EmbeddedCompoundLoftDirection::Vector(Vector3::new(0.0, 0.0, 1.0)),
        tail: RevisionCompoundLoftTail::Unbounded {},
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut out = AsmBrep::default();
    let definition = super::super::emit_revision_compound_loft_surface(
        &ctx,
        &mut out,
        7,
        embedded,
        crate::asm_format!("sat"),
    )
    .unwrap();
    let ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } = definition else {
        panic!("revision compound loft");
    };
    assert_eq!(construction.base_profile().len(), usize::from(present));
    assert!(construction.base_path().path.is_none());
    assert_eq!(construction.entries().len(), 1);
    assert_eq!(
        construction.entries()[0].profile.len(),
        usize::from(present)
    );
    assert!(construction.entries()[0].path.path.is_none());
    assert!(out.surfaces.is_empty());
    assert_eq!(out.curves.len(), if present { 2 } else { 0 });
    if present {
        for (profile, expected, curve) in [
            (
                &construction.base_profile()[0],
                "sat:brep:procedural_surface#7:cloft:base:profile:0",
                &out.curves[0],
            ),
            (
                &construction.entries()[0].profile[0],
                "sat:brep:procedural_surface#7:cloft:0:profile:0",
                &out.curves[1],
            ),
        ] {
            assert_eq!(profile.profile.id.as_str(), expected);
            assert_eq!(curve.id.as_str(), expected);
            assert_eq!(
                profile.profile.endpoints.map(|endpoints| endpoints
                    .map(|endpoint| endpoint.map(cadmpeg_ir::scalar::FiniteReal::get))),
                Some([Some(0.0), Some(1.0)])
            );
            assert!(matches!(
                profile.form,
                LoftMemberForm::PcurvePair {
                    pcurve: None,
                    secondary_pcurve: None,
                    asm_extension: None,
                    direction: None,
                    ..
                }
            ));
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn revision_compound_empty_profiles_keep_absent_paths() {
    revision_profiles(false);
}

#[test]
fn revision_compound_borrowed_profile_scopes_keep_native_identities() {
    revision_profiles(true);
}

fn support_copy_limits(refuse: bool) {
    use cadmpeg_core::decode::ResourceDimension;
    const SUPPORT_ID: &str = "sat:brep:procedural_surface#7:loft:0:0:support:0";
    let identity_bytes = u64::try_from(SUPPORT_ID.len()).unwrap();
    // Empty-vector growth retains one large slot or four ordinary slots.
    // Its old capacity is zero, so it moves no bytes. Only the ID copy has work.
    let slot_bytes = std::mem::size_of::<cadmpeg_ir::geometry::Surface>();
    let slots = if slot_bytes <= 1024 { 4 } else { 1 };
    let backing_bytes = u64::try_from(slots * slot_bytes).unwrap();
    for classic in [true, false] {
        for dimension in [
            ResourceDimension::WorkUnits,
            ResourceDimension::RetainedBytes,
        ] {
            let data = support(classic);
            let id = SurfaceId::mint(SUPPORT_ID).unwrap();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = identity_bytes;
            policy.limits.max_retained_bytes = backing_bytes + identity_bytes;
            policy.limits.max_collection_items = 1;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            if refuse {
                match dimension {
                    ResourceDimension::WorkUnits => policy.limits.max_work_units -= 1,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes -= 1,
                    _ => unreachable!("two copy dimensions"),
                }
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut out = AsmBrep::default();
            let result = super::super::emit_loft_member_form(&ctx, &mut out, data, || Ok(id));
            if !refuse {
                assert!(matches!(result.unwrap(), LoftMemberForm::Support {
                    surface: Some(surface), .. } if surface.as_str() == SUPPORT_ID));
                assert_eq!(out.surfaces.len(), 1);
                assert_eq!(out.surfaces[0].id.as_str(), SUPPORT_ID);
                assert!(out.surfaces[0].source_object.is_none());
                ctx.finish_session().unwrap();
                continue;
            }
            let cadmpeg_core::CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("copy limit refuses before surface insertion");
            };
            let used = if dimension == ResourceDimension::RetainedBytes {
                backing_bytes
            } else {
                0
            };
            assert_eq!(first.dimension, dimension);
            assert_eq!(first.operation, "ASM emitted identity copy");
            assert_eq!(
                (first.limit, first.used, first.additional),
                (used + identity_bytes - 1, used, identity_bytes)
            );
            assert!(out.surfaces.is_empty() && out.curves.is_empty());
            for _ in 0..64 {
                assert!(matches!(super::super::emit_loft_member_form(&ctx, &mut out,
                    support(classic), || panic!("original copy refusal skips factory")),
                    Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first));
                assert!(out.surfaces.is_empty() && out.curves.is_empty());
            }
            assert!(matches!(ctx.finish_session(),
                Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn loft_support_copy_accepts_exact_work_and_retained_limits() {
    support_copy_limits(false);
}

#[test]
fn loft_support_copy_refuses_one_short_and_keeps_original_refusal() {
    support_copy_limits(true);
}
