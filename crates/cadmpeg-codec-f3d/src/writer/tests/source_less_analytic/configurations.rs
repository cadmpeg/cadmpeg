// SPDX-License-Identifier: Apache-2.0
//! Neutral configuration regeneration and containment.

use super::*;
use crate::test_support::zip_test::f3d_with_configuration;

fn configuration_semantics(
    ir: &cadmpeg_ir::CadIr,
) -> Vec<cadmpeg_ir::features::DesignConfiguration> {
    let mut configurations = ir.model.configurations.clone();
    for configuration in &mut configurations {
        configuration.id = cadmpeg_ir::features::ConfigurationId::mint(format!(
            "test:f3d:configuration#{}",
            configuration.ordinal
        ))
        .unwrap();
        configuration.native_ref = None;
    }
    configurations.sort_by_key(|configuration| configuration.ordinal);
    configurations
}

/// An invalid configuration payload drops design intent and keeps geometry.
/// Strict mode still refuses the document with the parser's message.
fn assert_invalid_configuration_is_contained(name: &str, payload: &[u8], expected: &str) {
    let bytes = f3d_with_configuration(&synthetic_geometry_smbh(), name, payload);
    let contained = F3dCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("an invalid configuration is contained");
    let undecoded = contained
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::DesignIntentUndecoded.kind())
        .collect::<Vec<_>>();
    assert_eq!(undecoded.len(), 1, "{expected}");
    assert!(
        undecoded[0].message.contains(expected),
        "{}",
        undecoded[0].message
    );
    assert!(f3d_native(contained.ir()).design_configurations.is_empty());
    assert!(contained.ir().model.configurations.is_empty());
    assert!(!contained.ir().model.bodies.is_empty());

    let mut strict = DecodeOptions::default();
    strict.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    let refused = F3dCodec.decode(&mut Cursor::new(bytes), &strict);
    assert!(
        matches!(
            &refused,
            Err(cadmpeg_ir::DecodeFailure::StrictRejected { rejection })
                if rejection.loss().message.contains(expected)
        ),
        "{refused:?}"
    );
}

#[test]
fn generated_design_configuration_json_decodes_and_writes_source_less() {
    let name = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
    let payload = br#"{"configurations":{"Small":{},"Medium":{"parameters":{"width":"25 mm"},"suppressed":["slot"]},"Large":{}},"active":"Medium","extension":{"future":7}}"#;
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_configuration(
                    &synthetic_geometry_smbh(),
                    name,
                    payload,
                )),
                &DecodeOptions::default(),
            )
            .expect("generated configuration decode"),
    );
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_configurations.len(), 1);
    assert_eq!(native.design_configurations[0].entry_name(), name);
    assert_eq!(
        native.design_configurations[0].id().as_str(),
        format!("f3d:configuration:entry#{name}")
    );
    assert_eq!(
        native.design_configurations[0].kind(),
        crate::records::configuration::DesignConfigurationKind::Table
    );
    assert_eq!(
        native.design_configurations[0].variant_order(),
        ["Small", "Medium", "Large"]
    );
    assert_eq!(
        native.design_configurations[0].payload()["active"],
        "Medium"
    );
    assert_eq!(
        native.design_configurations[0].payload()["extension"]["future"],
        7
    );
    assert_eq!(decoded.ir().model.configurations.len(), 3);
    let mut authored = decoded
        .ir()
        .model
        .configurations
        .iter()
        .filter_map(|configuration| Some((configuration.name.as_deref()?, configuration.ordinal)))
        .collect::<Vec<_>>();
    authored.sort_by_key(|(_, ordinal)| *ordinal);
    assert_eq!(authored, [("Small", 0), ("Medium", 1), ("Large", 2)]);
    let medium = decoded
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Medium"))
        .expect("active medium configuration");
    assert!(medium.active);
    assert_eq!(medium.properties["parameter:width"], "25 mm");
    assert_eq!(medium.properties["suppressed:slot"], "true");
    assert_eq!(
        medium.native_ref.as_deref(),
        Some(native.design_configurations[0].id().as_str())
    );
    let mut retained = decoded.ir().clone();
    update_f3d_native(&mut retained, |native| {
        let configuration = &mut native.design_configurations[0];
        let mut payload = configuration.payload();
        payload["active"] = "Narrow".into();
        payload["configurations"]["Narrow"] =
            serde_json::json!({"parameters":{"width":"12 mm"},"suppressed":[]});
        let mut order = configuration.variant_order();
        order.push("Narrow".into());
        *configuration = crate::records::configuration::DesignConfiguration::try_new(
            configuration.entry_name().clone(),
            configuration.kind(),
            order,
            payload,
        )
        .unwrap();
    });
    retained.model.configurations = crate::design::configurations::project_configurations(
        &f3d_native(&retained).design_configurations,
    )
    .expect("edited configuration order");
    let expected_retained = configuration_semantics(&retained);
    let mut retained_bytes = Vec::new();
    crate::test_support::plan_inherited_write(
        &retained,
        decoded.source_fidelity(),
        &mut retained_bytes,
    )
    .expect("retained configuration edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained configuration round trip");
    assert_eq!(
        configuration_semantics(retained_round_trip.ir()),
        expected_retained
    );

    let expected_projected = configuration_semantics(decoded.ir());
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less configuration encode");
    let mut inconsistent = source_less.clone();
    inconsistent
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.name.as_deref() == Some("Medium"))
        .expect("active medium configuration")
        .active = false;
    let (updated, _) = synthesize_document(&inconsistent, None);
    assert_eq!(
        configuration_semantics(updated.ir()),
        configuration_semantics(&inconsistent)
    );
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less configuration round trip");
    assert_eq!(configuration_semantics(round_trip.ir()), expected_projected);

    let rule_name = "FusionAssetName[Active]/DesignConfigurationRule.456.dsgcfgrule";
    let rule_result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                rule_name,
                br#"{"when":"width > 20 mm","activate":"wide"}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("generated configuration-rule decode");
    assert!(rule_result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "configuration rule(s) were retained without an unambiguous neutral activation target"
        )));
    let rule = f3d_native(rule_result.ir()).design_configurations.remove(0);
    assert_eq!(
        rule.kind(),
        crate::records::configuration::DesignConfigurationKind::Rule
    );
    assert_eq!(rule.payload()["activate"], "wide");

    assert_invalid_configuration_is_contained(name, b"[]", "configuration JSON must be an object");

    for (payload, expected) in [
        (
            br#"{"configurations":{"wide":{}},"active":"missing"}"#.as_slice(),
            "is not a named variant",
        ),
        (
            br#"{"configurations":{"wide":{"parameters":[]}}}"#.as_slice(),
            "parameters must be an object",
        ),
        (
            br#"{"configurations":{"wide":{"suppressed":[7]}}}"#.as_slice(),
            "suppressed list must contain strings",
        ),
        (
            br#"{"configurations":{"wide":{"material":7}}}"#.as_slice(),
            "material must be a string",
        ),
    ] {
        assert_invalid_configuration_is_contained(name, payload, expected);
    }

    let partial_rule = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                rule_name,
                br#"{"when":"width > 20 mm","vendorExtension":7}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("partial native rule remains decodable");
    assert!(partial_rule.ir().model.configurations.is_empty());
    let partial_native = f3d_native(partial_rule.ir());
    assert_eq!(
        partial_native.design_configurations[0].payload()["vendorExtension"],
        7
    );
    assert!(partial_rule
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "configuration rule(s) were retained without an unambiguous neutral activation target"
        )));
}
