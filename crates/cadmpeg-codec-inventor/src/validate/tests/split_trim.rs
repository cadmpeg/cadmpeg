// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{
    features::{
        BodySelection, BodyTrimSide, FeatureDefinition, FeatureEvaluation, FeatureOperation,
        FeatureResultMembers, FeatureResultTopology, TrimBodyOperands,
    },
    ids::FeatureResultTopologyId,
};

use super::{annotation, empty_data, TestResult};
use crate::{
    feature::{
        split::CLASS_ID,
        tests::{reference, test_label},
        PmDcFeaturePropertyKind,
    },
    test_support::split::{boolean, set_kind, set_marker, set_slot, set_version, trim_fixture},
};

#[test]
fn surface_trim_validation_requires_the_bounded_operands_and_native_side() -> TestResult {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    for changed in std::iter::once(None).chain((0..=44).map(Some)) {
        let (mut source, mut properties) = trim_fixture();
        let mut keep = BodyTrimSide::Unresolved;
        match changed {
            Some(case @ 0..=9) => {
                let old = source.properties.references()[case];
                set_slot(
                    &mut source,
                    case,
                    if old.index() == 0 {
                        reference(62)
                    } else {
                        reference(0)
                    },
                );
            }
            Some(10) => set_version(&mut source, 20),
            Some(case @ 11..=18) => {
                properties[[0, 1, 2, 3, 4, 5, 7, 10][case - 11]]
                    .identity
                    .type_id = crate::record_identity::RecordTypeId::from_bytes([0; 16]);
            }
            Some(19) => {
                source.identity.type_id = crate::record_identity::RecordTypeId::from_bytes([0; 16]);
            }
            Some(case @ 20..=29) => {
                let slot = case - 20;
                let old = source.properties.references()[slot];
                set_slot(
                    &mut source,
                    slot,
                    crate::pmdc::PmDcReference::new(old.index(), !old.qualified())
                        .ok_or("reference")?,
                );
            }
            Some(case @ 30..=31) => set_marker(&mut properties, [3, 5][case - 30]),
            Some(32) => set_kind(&mut properties, 4, boolean(true)),
            Some(33) => set_kind(&mut properties, 1, boolean(true)),
            Some(34) => keep = BodyTrimSide::Forward,
            Some(35) => keep = BodyTrimSide::Reverse,
            Some(36) => set_slot(&mut source, 8, reference(69)),
            Some(case @ 37..=40) => {
                let position = if case.is_multiple_of(2) { 10 } else { 7 };
                let body = if case < 39 {
                    reference(1)
                } else {
                    reference(0)
                };
                set_kind(
                    &mut properties,
                    position,
                    PmDcFeaturePropertyKind::SurfaceBody { body },
                );
            }
            Some(case @ 41..=44) => {
                let position = if case.is_multiple_of(2) { 4 } else { 1 };
                set_kind(
                    &mut properties,
                    position,
                    PmDcFeaturePropertyKind::Boolean {
                        name: if case >= 43 {
                            "control".into()
                        } else {
                            String::new()
                        },
                        name_value: u32::from(case < 43),
                        value: false,
                    },
                );
            }
            None => {}
            Some(_) => return Err("fixture mutation".into()),
        }
        let mut feature = annotation(source.id());
        feature.id = crate::feature::history_feature_id(&source.identity)?;
        feature.source_tag = Some("trim_bodies".into());
        feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::TrimBodies {
                operands: TrimBodyOperands::new(
                    BodySelection::Native(properties[7].id()),
                    BodySelection::Native(properties[10].id()),
                )?,
                keep,
            },
        ));
        let result = FeatureResultTopology::new(
            FeatureResultTopologyId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                cadmpeg_ir::identity_key!("surface-trim"),
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
        data.pm_dc_feature_labels = vec![test_label(30, 4, CLASS_ID, &[])];
        for (ordinal, type_id) in data
            .pm_dc_features
            .iter()
            .map(|x| &x.identity)
            .chain(data.pm_dc_feature_properties.iter().map(|x| &x.identity))
            .chain(data.pm_dc_feature_labels.iter().map(|x| &x.identity))
            .map(|identity| (identity.record_ordinal, identity.type_id.as_str()))
            .chain(std::iter::once((2, "00000000000000000000000000000000")))
        {
            data.records.push(serde_json::from_value(serde_json::json!({
                "id":format!("inventor:rse:record#generated-{ordinal}"), "token":"generated", "ordinal":ordinal, "selector":0, "type_index":0,
                "type_id":type_id, "payload_offset":0, "payload_len":0, "payload_sha256":"", "trailing_payload_len":0, "trailer_len":0, "trailer_sha256":""
            }))?);
        }
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.features = vec![feature];
        ir.model.feature_result_topologies = vec![result];
        let mut findings = Vec::new();
        super::super::validate_features(&ctx, &ir, &data, &mut findings)?;
        if changed.is_none() {
            assert!(findings.is_empty(), "{findings:?}");
        }
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.message == "Inventor neutral split does not match its PmDc operands")
                .count(),
            usize::from(changed.is_some()),
            "case {changed:?}; {findings:?}"
        );
    }
    Ok(())
}
