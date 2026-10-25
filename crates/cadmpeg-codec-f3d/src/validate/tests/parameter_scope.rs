// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    crate::native::F3dNative {
        design_parameter_scopes: vec![DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        )],
        ..Default::default()
    }
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design parameter scope has an invalid paired frame"
        );
    })
}

#[test]
fn placement_ordered_owner_count_and_references_match_the_frame() {
    use crate::records::feature::{
        assembly::{DesignAssemblyAlignment, DesignAssemblyOwnerRun},
        scope::{DesignFeatureKind, DesignParameterScope},
    };
    use crate::records::identity::{Located, ReferenceRun};
    let owners = [(0, 50), (1, 51), (2, 52), (3, 53), (4, 54), (5, 55)];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::AsBuilt,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 422;
            draft.paired_byte_offset = draft.byte_offset + 422;
            draft.reference_members =
                ReferenceRun::unlocated(vec![80, 50, 51, 52, 53, 81, 82, 83, 84, 54, 55]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut alignment = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        (50..54).map(|value| Located { value, offset: 0 }).collect(),
        None,
    )
    .unwrap();
    alignment.additional_owner_run = Some(DesignAssemblyOwnerRun {
        count: Located {
            value: 2,
            offset: scope.byte_offset() + 43,
        },
        references: vec![
            Located {
                value: 54,
                offset: scope.byte_offset() + 47,
            },
            Located {
                value: 55,
                offset: scope.byte_offset() + 58,
            },
        ],
    });
    let holds = |alignment: &DesignAssemblyAlignment| {
        super::super::placement_ordered_references_hold(&scope, alignment, &owners)
    };
    assert!(holds(&alignment));
    let mut absent = alignment.clone();
    absent.additional_owner_run = None;
    assert!(!holds(&absent));
    for value in [0, 1, 3] {
        let mut changed = alignment.clone();
        changed.additional_owner_run.as_mut().unwrap().count.value = value;
        assert!(!holds(&changed));
    }
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[1].value = 54;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[0].offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().count.offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed
        .additional_owner_run
        .as_mut()
        .unwrap()
        .references
        .pop();
    assert!(!holds(&changed));
    assert!(!super::super::placement_ordered_references_hold(
        &scope,
        &alignment,
        &owners[..5]
    ));
}

#[test]
fn validation_accepts_malformed_rest_with_retained_limits() {
    use super::{
        f3d_native_mut, f3d_with_smbh_and_protein, synthetic_geometry_smbh, Codec, Cursor,
        DecodeOptions, F3dCodec,
    };
    use crate::records::feature::joint_motion::{DesignJointLimitLanes, DesignJointMotion};
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    let before = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    f3d_native_mut(&mut ir)
        .design_joint_motions
        .push(DesignJointMotion {
            id: "f3d:Design/BulkStream.dat:design-joint-motion#12".into(),
            joint_record_index: 10,
            joint_byte_offset: 0,
            motion_record_index: 12,
            motion_byte_offset: 100,
            freedoms: vec![crate::records::identity::Located {
                value: 8,
                offset: 150,
            }],
            entry_words: Vec::new(),
            limits: Some(DesignJointLimitLanes {
                flags_offset: 42,
                first_flag: 1,
                second_flag: 1,
                maximum: 1.5,
                minimum: -1.5,
                values_offset: 54,
                rest: None,
                rest_malformed: true,
            }),
        });
    let after = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    assert_eq!(before, after);
}
