// SPDX-License-Identifier: Apache-2.0

use crate::brep::{AsmBrep, Reachable};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ScopedReservation,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

fn index_source_refusal(mut out: AsmBrep, records: &[crate::sab::Record], operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let mut storage = ctx
        .reserve_scoped(0, "test attribute owner")
        .expect("owner");
    let before = serde_json::to_value(&out).expect("output snapshot");
    let run =
        |out: &mut AsmBrep, records: &[crate::sab::Record], storage: &mut ScopedReservation<'_>| {
            if operation == "ASM record reference scan" {
                super::super::count_other_records(
                    &ctx,
                    out,
                    records,
                    &Reachable::default(),
                    &std::collections::HashSet::new(),
                )
            } else {
                super::super::emit_attributes(
                    &ctx,
                    out,
                    records,
                    &std::collections::HashMap::new(),
                    &Reachable::default(),
                    crate::asm_format!("sat"),
                    storage,
                )
                .map(|_| ())
            }
        };
    let Err(CodecError::ResourceLimit(first)) = run(&mut out, records, &mut storage) else {
        panic!("first actual index source visit must refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.used, first.additional, first.limit), (0, 1, 0));
    assert_eq!(serde_json::to_value(&out).expect("output snapshot"), before);
    for _ in 0..64 {
        assert!(matches!(run(&mut out, records, &mut storage),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(serde_json::to_value(&out).expect("output snapshot"), before);
    }
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn asm_attribute_body_index_refuses_first_source_visit() {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};
    let mut out = AsmBrep::default();
    out.bodies.push(Body {
        id: BodyId::mint("sat:brep:body#1").expect("body identity"),
        kind: BodyKind::Wire,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    index_source_refusal(out, &[], "ASM attribute body ids");
}

#[test]
fn asm_attribute_shell_index_refuses_first_source_visit() {
    use cadmpeg_ir::ids::{FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::Shell;
    let mut out = AsmBrep::default();
    out.shells.push(Shell::with_face(
        ShellId::mint("sat:brep:shell#1").expect("shell identity"),
        RegionId::mint("sat:brep:region#1").expect("region identity"),
        FaceId::mint("sat:brep:face#1").expect("face identity"),
    ));
    index_source_refusal(out, &[], "ASM attribute shell ids");
}

#[test]
fn asm_record_reference_indices_refuse_first_source_visit() {
    let records = [crate::test_support::sab::record(0, "body".into(), Vec::new().into(), 0, 0)
            .expect("indexed record fixture")];
    index_source_refusal(AsmBrep::default(), &records, "ASM record reference scan");
}

#[test]
fn asm_record_reference_indices_keep_transform_and_intcurve_membership() {
    use crate::sab::Token;
    use std::collections::{BTreeMap, HashSet};
    let body = [
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(1),
    ];
    let pcurve = [
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(3),
    ];
    let records = [
        crate::test_support::sab::record(0, "body".into(), body.to_vec().into(), 0, 0)
            .expect("indexed record fixture"),
        crate::test_support::sab::record(1, "opaque-transform".into(), Vec::new().into(), 0, 0)
            .expect("indexed record fixture"),
        crate::test_support::sab::record(2, "opaque-pcurve".into(), pcurve.to_vec().into(), 0, 0)
            .expect("indexed record fixture"),
        crate::test_support::sab::record(3, "opaque-intcurve".into(), Vec::new().into(), 0, 0)
            .expect("indexed record fixture"),
        crate::test_support::sab::record(4, "opaque-other".into(), Vec::new().into(), 0, 0)
            .expect("indexed record fixture"),
    ];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let mut out = AsmBrep::default();
    let reach = Reachable {
        pcurves: HashSet::from([2]),
        ..Reachable::default()
    };
    super::super::count_other_records(&ctx, &mut out, &records, &reach, &HashSet::new())
        .expect("reference classification");
    assert_eq!(
        out.stats.other_record_kinds,
        BTreeMap::from([("opaque-other".into(), 1)])
    );
    ctx.finish_session()
        .expect("successful reference classification");
}

#[test]
fn asm_absent_loft_member_keeps_support_type_one() {
    use crate::nurbs::proc_surface::LoftProfileData;
    use cadmpeg_ir::geometry::{LoftMemberForm, LoftSubdata};
    crate::test_support::with_zero_context(|ctx| {
        let mut out = AsmBrep::default();
        let subdata = LoftSubdata::Type211 {
            dimensions: [0, 0],
            row: [0.0, 1.0],
        };
        let data = LoftProfileData::RevisionSupport {
            endpoints: [None; 2],
            type_code: std::num::NonZeroI64::new(1).expect("nonzero support discriminator"),
            surface: None,
            support_bounds: [None; 4],
            pcurve: None,
            first_flag: false,
            asm_extension: None,
            subdata,
            direction: None,
        };
        let result = super::super::emit_loft_member_form(ctx, &mut out, data, || {
            panic!("absent or refused support executes no identity factory")
        });
        assert!(matches!(
            result,
            Ok(LoftMemberForm::Support {
                type_code: 1,
                surface: None,
                pcurve: None,
                ..
            })
        ));
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_absent_rolling_ball_side_keeps_zero_curve_kind() {
    use cadmpeg_ir::geometry::{RollingBallSide, VariableBlendSupportKind};
    crate::test_support::with_zero_context(|ctx| {
        let mut out = AsmBrep::default();
        let side = RollingBallSide {
            support_kind: VariableBlendSupportKind::ZeroCurve,
            surface: None,
            curve: None,
            pcurve: None,
            location: Point3::new(0.0, 0.0, 0.0),
            secondary_pcurve: None,
            extension: None,
        };
        let result = super::super::emit_rolling_ball_side(
            ctx,
            &mut out,
            crate::asm_format!("sat"),
            || panic!("absent or refused rolling side skips identity"),
            side,
        );

        let side = result.expect("absent side is free");
        assert!(side.surface.is_none() && side.curve.is_none() && side.pcurve.is_none());
        assert_eq!(side.support_kind, VariableBlendSupportKind::ZeroCurve);

        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}

#[test]
fn asm_spring_support_keeps_exact_ranges() {
    let ranges = [[0.0, 1.0], [0.0, 1.0]];
    crate::test_support::with_zero_context(|ctx| {
        let mut out = AsmBrep::default();
        let result = super::super::emit_spring_support(
            ctx,
            &mut out,
            7,
            crate::asm_format!("sat"),
            0,
            crate::nurbs::proc_curve::EmbeddedSpringSupport::Ranges(ranges),
        );
        assert!(
            matches!(result, Ok(cadmpeg_ir::geometry::SpringSupport::Ranges(value)) if value == ranges)
        );
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
    });
}
