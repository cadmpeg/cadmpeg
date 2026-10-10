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
                assert_eq!(angle.get(), 1.0);
            }
            (Some(FlexForm::Tapering), FlexMode::Tapering { factor }) => {
                assert_eq!(factor.get(), 2.0);
            }
            (Some(FlexForm::Stretching), FlexMode::Stretching { distance }) => {
                assert_eq!(distance.get(), 1.0);
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

fn two_variable_radii() -> crate::records::Feature {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    for (name, value) in [
        ("Position0", "0"),
        ("Position1", "1"),
        ("Radius0", "2mm"),
        ("Radius1", "3mm"),
    ] {
        source
            .parameters
            .insert(name.try_into().unwrap(), value.into());
    }
    source
}

fn variable_radius_storage() -> (u64, u64, u64) {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;
    use cadmpeg_ir::scalar::{Fraction, NonNegativeLength};
    // Two distinct positions admit one B-tree node: eleven key/value slots,
    // sixteen metadata/child pointers and two alignment paddings.
    let alignment = std::mem::align_of::<usize>().max(std::mem::align_of::<&String>());
    let positions = 11 * (std::mem::size_of::<usize>() + std::mem::size_of::<&String>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * alignment;
    type Radius = VariableRadius<Fraction, NonNegativeLength>;
    // Both vectors have the first four-slot amortized growth; input tuples
    // remain live through their consuming iterator, after the table retires.
    let points = 4 * std::mem::size_of::<(usize, Radius)>();
    let output = 4 * std::mem::size_of::<Radius>();
    (
        cadmpeg_core::decode::u64_from_index(positions),
        cadmpeg_core::decode::u64_from_index(points),
        cadmpeg_core::decode::u64_from_index(output),
    )
}

#[test]
fn variable_fillet_retires_position_table_before_output_controls() {
    let source = two_variable_radii();
    let (positions, points, output) = variable_radius_storage();
    let need = positions + points;
    assert!(points + output <= need);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = need;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let storage;
        let radii;
        (radii, storage) = ctx
            .with_scoped_storage("hold test variable radii", || {
                Ok::<_, cadmpeg_core::CodecError>(
                    super::variable_radius_points(&ctx, &source)?.unwrap(),
                )
            })
            .unwrap();
        assert_eq!(radii.len(), 2);
        assert_eq!(radii[0].parameter.get(), 0.0);
        assert_eq!(radii[1].parameter.get(), 1.0);
        assert_eq!(radii[0].radius.get(), 2.0);
        assert_eq!(radii[1].radius.get(), 3.0);
        let reuse = ctx
            .reserve_scoped(need - output, "reuse retired fillet table")
            .unwrap();
        drop(reuse);
        drop((radii, storage));
        let reuse = ctx
            .reserve_scoped(need, "reuse all fillet controls storage")
            .unwrap();
        drop(reuse);
    }
}

#[test]
fn variable_fillet_control_growth_preserves_exact_original_refusal() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let source = two_variable_radii();
    let (positions, points, _) = variable_radius_storage();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = positions + points - 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::variable_radius_points(&ctx, &source).unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected variable control growth refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "collect SLDPRT variable fillet radii");
    assert_eq!(first.used, positions);
    assert_eq!(first.additional, points);
    assert_eq!(ctx.resource_refusal(), Some(first.clone()));
    let CodecError::ResourceLimit(next) =
        ctx.reserve_scoped(0, "sticky fillet refusal").unwrap_err()
    else {
        panic!("expected original variable control refusal");
    };
    assert_eq!(next, first);
}
