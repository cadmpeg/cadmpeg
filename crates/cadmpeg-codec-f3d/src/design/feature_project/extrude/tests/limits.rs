// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::extrude::project_extrude;
use crate::design::feature_project::extrude::test_support::{profile_group,scope,scope_with_extent};
use crate::design::test_support::parameter_record;
use crate::records::feature::extrude::DesignExtrudeExtent;

use crate::records::topology::construction::{DesignConstructionOperandGroup,DesignConstructionOperandRole};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};


fn parameters() -> [crate::records::parameters::DesignParameter; 2] {
    [
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "AlongDistance",
            Some("mm"),
            "d1",
            0.55,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "TaperAngle",
            Some("deg"),
            "d2",
            0.2,
        ))
        .unwrap(),
    ]
}

fn assert_profile_fallback(
    groups: &[DesignConstructionOperandGroup],
    expected_native: &str,
    operation: &'static str,
) {
    let scope = scope();
    let parameters = parameters();
    let owned = [(0, &parameters[0]), (1, &parameters[1])];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)), ..
        }) if native == expected_native
    ));
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_extrude(&ctx, &scope, &owned, groups, &[], &[], &[])).map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn extrude_single_profile_group_id_refuses_retained_limit() {
    let first = profile_group(100, 0);
    assert_profile_fallback(
        std::slice::from_ref(&first),
        &first.id,
        "f3d Extrude profile group id",
    );
}

#[test]
fn extrude_multiple_profile_fallback_scope_id_refuses_retained_limit() {
    let groups = [profile_group(100, 0), profile_group(101, 1)];
    assert_profile_fallback(
        &groups,
        "f3d:Design/BulkStream.dat:scope#12",
        "f3d Extrude fallback scope id",
    );
}

fn assert_face_fallback(role: DesignConstructionOperandRole, operation: &'static str) {
    let scope = scope_with_extent(DesignExtrudeExtent::OneSidedToFace);
    let profile = profile_group(100, 0);
    let mut selection = profile_group(101, 1);
    selection.operand_role = role;
    let groups = [profile, selection];
    let side_offset = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side1Offset",
        Some("mm"),
        "d3",
        0.0,
    ))
    .unwrap();
    let taper = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "TaperAngle",
        Some("deg"),
        "d2",
        0.2,
    ))
    .unwrap();
    let owned = [(0, &side_offset), (1, &taper)];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, &groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    ));
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_extrude(&ctx, &scope, &owned, &groups, &[], &[], &[])).map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn extrude_selected_face_group_id_refuses_retained_limit() {
    assert_face_fallback(
        DesignConstructionOperandRole::ExtrudeFaces {
            encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
            usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
        },
        "f3d Extrude selected face group id",
    );
}

#[test]
fn extrude_target_shape_group_id_refuses_retained_limit() {
    assert_face_fallback(
        DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
        ),
        "f3d Extrude target shape group id",
    );
}
