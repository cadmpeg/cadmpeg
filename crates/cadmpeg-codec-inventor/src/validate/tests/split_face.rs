// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    FaceSelection, FeatureDefinition, FeatureEvaluation, FeatureOperation, FeatureResultMembers,
    FeatureResultTopology, SplitFaceTool,
};

use cadmpeg_ir::ids::FeatureResultTopologyId;

use super::{annotation, empty_data, TestResult};
use crate::feature::{
    split::CLASS_ID,
    tests::{reference, test_label},
    work_plane::{frame, tests::work_plane},
};
use crate::test_support::split::{
    face_fixture as fixture, set_kind, set_marker, set_slot, set_version,
};

#[test]
fn face_split_validation_recomputes_both_controls_and_all_operand_slots() -> TestResult {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    for side in [false, true] {
        for changed_slot in std::iter::once(None).chain((0..=38).map(Some)) {
            let (mut source, mut properties) = fixture(side);
            let plane = work_plane(42, [0.0, 0.0, 1.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
            let mut datum = annotation(plane.id());
            datum.id = crate::feature::history_feature_id(&plane.identity)?;
            datum.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::DatumPlane {
                    frame: frame(&plane).ok_or("plane frame")?,
                },
            ));
            if let Some(case @ 35..=38) = changed_slot {
                let position = [1, 4][(case - 35) / 2];
                let mut kind = properties[position].kind.clone();
                let crate::feature::PmDcFeaturePropertyKind::Boolean {
                    name, name_value, ..
                } = &mut kind
                else {
                    return Err("Boolean fixture".into());
                };
                if case.is_multiple_of(2) {
                    *name = "control".into();
                } else {
                    *name_value = 1;
                }
                set_kind(&mut properties, position, kind);
            } else if changed_slot == Some(34) {
                source.identity.type_id = crate::record_identity::RecordTypeId::from_bytes([0; 16]);
            } else if let Some(case @ 30..=33) = changed_slot {
                set_marker(&mut properties, [3, 5, 11, 12][case - 30]);
            } else if let Some(case @ 20..=29) = changed_slot {
                let slot = case - 20;
                let reference = source.properties.references()[slot];
                set_slot(
                    &mut source,
                    slot,
                    crate::pmdc::PmDcReference::new(reference.index(), !reference.qualified())
                        .ok_or("reference")?,
                );
            } else if let Some(case @ 11..=19) = changed_slot {
                let position = [0, 1, 2, 3, 4, 5, 7, 11, 12][case - 11];
                properties[position].identity.type_id =
                    crate::record_identity::RecordTypeId::from_bytes([0; 16]);
            } else if changed_slot == Some(10) {
                set_version(&mut source, 20);
            } else if let Some(slot) = changed_slot {
                let value = if source.properties.references()[slot].index() == 0 {
                    reference(2)
                } else {
                    reference(0)
                };
                set_slot(&mut source, slot, value);
            }
            let mut feature = annotation(source.id());
            feature.id = crate::feature::history_feature_id(&source.identity)?;
            feature.ordinal = 1;
            feature.source_tag = Some("split_face".into());
            feature.dependencies.insert(datum.id.clone());
            feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::SplitFace {
                    targets: FaceSelection::Native(properties[11].id()),
                    tool: SplitFaceTool::Plane {
                        plane: datum.id.clone(),
                    },
                },
            ));
            let result = FeatureResultTopology::new(
                FeatureResultTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                    cadmpeg_ir::identity_key!("face-split"),
                ),
                feature.id.clone(),
                FeatureResultMembers::new(
                    vec![cadmpeg_core::text::NonBlankString::new(properties[7].id())
                        .ok_or("body identity")?],
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )?,
                Some(properties[5].id()),
            );
            let mut data = empty_data();
            data.pm_dc_features = vec![source];
            data.pm_dc_feature_properties = properties;
            data.pm_dc_work_planes = vec![plane];
            data.pm_dc_feature_labels = vec![test_label(30, 4, CLASS_ID, &[])];
            for (ordinal, type_id) in data
                .pm_dc_features
                .iter()
                .map(|r| &r.identity)
                .chain(data.pm_dc_feature_properties.iter().map(|r| &r.identity))
                .chain(data.pm_dc_feature_labels.iter().map(|r| &r.identity))
                .chain(data.pm_dc_work_planes.iter().map(|r| &r.identity))
                .map(|identity| (identity.record_ordinal, identity.type_id.as_str()))
                .chain(std::iter::once((2, "00000000000000000000000000000000")))
            {
                data.records.push(serde_json::from_value(serde_json::json!({
                    "id": format!("inventor:rse:record#generated-{ordinal}"),
                    "token": "generated", "ordinal": ordinal, "selector": 0, "type_index": 0,
                    "type_id": type_id,
                    "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
                    "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
                }))?);
            }
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.features = vec![datum, feature];
            ir.model.feature_result_topologies = vec![result];
            let mut findings = Vec::new();
            super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
            let mismatch = findings
                .iter()
                .filter(|f| f.message == "Inventor neutral split does not match its PmDc operands")
                .count();
            if changed_slot.is_none() {
                assert!(findings.is_empty(), "{findings:?}");
            }
            assert_eq!(
                mismatch,
                usize::from(changed_slot.is_some()),
                "side {side}; slot {changed_slot:?}; {findings:?}"
            );
        }
    }
    Ok(())
}
