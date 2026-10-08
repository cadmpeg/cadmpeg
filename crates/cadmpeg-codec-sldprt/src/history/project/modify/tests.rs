// SPDX-License-Identifier: Apache-2.0
//! Fillet rejection and mode-specific Flex operand work.

use crate::history::project::budget_tests::{limited, property};
use crate::history::tests::feature;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn missing_fillet_position_leaves_remaining_radii_pass_unpaid() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Radius0"), "2mm".into());
    for index in 0..12_000 {
        source
            .parameters
            .insert(format!("z{index:05}").try_into().unwrap(), "unused".into());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 20_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let definition = super::project_fillet(&ctx, &source).unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(groups[0].radius, cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) }))
    );
    crate::test_support::work_refusal_at("scan SLDPRT variable fillet radii", |ctx| {
        super::project_fillet(ctx, &source)
    });
}

#[test]
fn flex_reads_only_the_operand_of_its_selected_form() {
    use cadmpeg_ir::features::{FlexForm, FlexMode};
    for (mode, required, value, form) in [
        ("bending", "Angle", "1rad", Some(FlexForm::Bending)),
        ("twisting", "Angle", "1rad", Some(FlexForm::Twisting)),
        ("tapering", "Factor", "2", Some(FlexForm::Tapering)),
        ("stretching", "Distance", "1mm", Some(FlexForm::Stretching)),
        ("invalid", "", "", None),
    ] {
        let mut source = feature("flex", None, 0);
        property(&mut source, "Mode", mode.into());
        for name in ["Angle", "Factor", "Distance"] {
            source.parameters.insert(
                name.try_into().unwrap(),
                if name == required {
                    value.into()
                } else {
                    "x".repeat(100_000)
                },
            );
        }
        let definition = limited(|ctx| super::project_flex(ctx, &source)).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Flex { mode, .. }) = definition else {
            panic!("expected flex");
        };
        match (form, mode) {
            (Some(FlexForm::Bending), FlexMode::Bending { angle })
            | (Some(FlexForm::Twisting), FlexMode::Twisting { angle }) => {
                assert_eq!(angle.get(), 1.0)
            }
            (Some(FlexForm::Tapering), FlexMode::Tapering { factor }) => {
                assert_eq!(factor.get(), 2.0)
            }
            (Some(FlexForm::Stretching), FlexMode::Stretching { distance }) => {
                assert_eq!(distance.get(), 1.0)
            }
            (None, FlexMode::Unresolved { form: None }) => {}
            (_, mode) => panic!("unexpected flex mode: {mode:?}"),
        }
        if let Some(form) = form {
            source.parameters.remove(required);
            assert!(
                matches!(limited(|ctx| super::project_flex(ctx, &source)).unwrap(), FeatureDefinition::Operation(FeatureOperation::Flex { mode: FlexMode::Unresolved { form: Some(actual) }, .. }) if actual == form)
            );
            source
                .parameters
                .insert(required.try_into().unwrap(), "invalid".into());
            assert!(
                matches!(limited(|ctx| super::project_flex(ctx, &source)).unwrap(), FeatureDefinition::Operation(FeatureOperation::Flex { mode: FlexMode::Unresolved { form: Some(actual) }, .. }) if actual == form)
            );
        }
    }
}

