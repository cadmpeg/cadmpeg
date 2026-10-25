// SPDX-License-Identifier: Apache-2.0
//! The class-397 symmetric-distance Extrude frame.

use super::*;

#[test]
fn validation_accepts_only_the_class_397_symmetric_extent_frame() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            extrude::{
                DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
                DesignExtrudeStart,
            },
            scope::{DesignExtrudeScope, DesignFeatureKind, DesignParameterScope},
        },
        identity::ReferenceRun,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#0");
    let mut scope = DesignParameterScope::empty(&scope_id, DesignFeatureKind::Extrude, 3970);
    scope.class_tag = "397".to_owned().try_into().unwrap();
    scope.paired_class_tag = "262".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 473;
            draft.paired_byte_offset = 473;
            draft.reference_count_offset = 282;
            draft.reference_members = ReferenceRun::from_columns(
                vec![11, 22, 33, 44, 55, 66, 77, 88],
                vec![287, 298, 309, 320, 331, 342, 353, 364],
                "reference_members",
            )
            .unwrap();
            draft.kind_offset = 382;
            draft.feature_ordinal_offset = 395;
            draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::LegacyShifted {
                    operation_prefix_marker: None,
                    operation: DesignExtrudeOperation::Cut,
                    operation_offset: Some(27),
                    direction_face_extend_values: [3, 2],
                    side_extent_discriminators: [1, 1],
                    side_extent_discriminator_offsets: Some([126, 139]),
                    extent: Some(DesignExtrudeExtent::SymmetricDistance),
                    direction_face_extend_offsets: Some([31, 35]),
                    direction_reversed: false,
                    direction_reversed_offset: Some(39),
                    solid_operation: true,
                    solid_operation_offset: Some(40),
                    start: DesignExtrudeStart::OffsetProfilePlane,
                    start_offset: Some(41),
                }),
                ..DesignExtrudeScope::default()
            }));
        })
        .unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        for record_index in [11, 22, 33, 44, 55, 66, 77, 88, 3970] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{record_index}"),
                record_index,
                class_tag: "397".to_owned().try_into().unwrap(),
                byte_offset: Some(0),
            });
        }
        native.design_parameter_scopes.push(scope.clone());
    }
    // The scope capture states the source offsets of the located scope.
    let capture = |scope: &DesignParameterScope| {
        let pointers = crate::records::source_offsets::collect(&mut scope.clone());
        let pointers = pointers
            .iter()
            .map(|(name, offset)| (name.as_str(), *offset))
            .collect::<Vec<_>>();
        crate::test_support::source_layout::sidecar(
            "Design/BulkStream.dat",
            &[(scope_id.as_str(), 0, 473, &pointers[..])],
        )
    };
    let has_invalid_frame = |ir: &cadmpeg_ir::CadIr, scope: &DesignParameterScope| {
        let fidelity = capture(scope);
        crate::test_support::with_decode_context(|ctx| {
            let mut findings =
                crate::validate::validate_native(ctx, ir).expect("validate the native namespace");
            findings.extend(
                crate::validate::source_layout::validate_source_layout(ctx, ir, &fidelity)
                    .expect("validate the source layout"),
            );
            findings
        })
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some(scope_id.as_str())
                && finding.message == "Fusion Design parameter scope has an invalid paired frame"
        })
    };
    assert!(!has_invalid_frame(&ir, &scope));

    let mutations: &[fn(&mut DesignParameterScope)] = &[
        |scope| scope.class_tag = "338".to_owned().try_into().unwrap(),
        |scope| scope.paired_class_tag = "261".to_owned().try_into().unwrap(),
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminators,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminators = [1, 0];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                direction_face_extend_values,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *direction_face_extend_values = [3, 1];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminator_offsets,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminator_offsets = Some([116, 129]);
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
                scope.extrude_prologue_mut()
            {
                *extent = Some(DesignExtrudeExtent::TwoSidedDistance);
            }
        },
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let mut invalid = scope.clone();
        mutate(&mut invalid);
        f3d_native_mut(&mut ir).design_parameter_scopes[0] = invalid.clone();
        assert!(
            has_invalid_frame(&ir, &invalid),
            "accepted invalid frame mutation {index}"
        );
    }
}
