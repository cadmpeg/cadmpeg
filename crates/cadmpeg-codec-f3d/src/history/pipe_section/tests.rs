// SPDX-License-Identifier: Apache-2.0

use super::{bind_circular_pipe_sections, PipeSectionInputs};
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::{DesignPipeConstruction, DesignPipeSection};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    GeneratedSweepSection, PathRef, SweepSection, SweepShape,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossKind;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const LINEAR_TOLERANCE: f64 = 1.0e-6;

fn projected_fixture(
    fixture: &crate::test_support::pipe::Fixture,
    edit: impl FnOnce(&mut crate::native::F3dNative, &mut Feature),
) -> (Feature, Vec<LossKind>) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut native = fixture.decode(&ctx).unwrap();
    let ir = result_model(&[]);
    let mut feature = pipe_feature(&native.design_parameter_scopes[0], &ir);
    feature.source_properties.insert(
        cadmpeg_core::text::NonBlankString::new("reference:0").unwrap(),
        "100".into(),
    );
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Pipe".into(),
            parameters: [(
                cadmpeg_core::text::NonBlankString::new("D").unwrap(),
                "6 mm".into(),
            )]
            .into(),
        }));
    edit(&mut native, &mut feature);
    fixture.with_scan(|scan| {
        let inputs = PipeSectionInputs {
            scopes: &native.design_parameter_scopes,
            groups: &native.design_construction_operand_groups,
            owners: &native.design_parameter_owners,
            bodies: &ir.model.bodies,
            regions: &ir.model.regions,
            faces: &ir.model.faces,
            surfaces: &ir.model.surfaces,
            linear_tolerance: LINEAR_TOLERANCE,
        };
        let mut features = vec![feature];
        let losses = super::projection::project(
            scan,
            &native.design_types,
            &native.design_record_headers,
            &mut features,
            &inputs,
        )
        .unwrap();
        let first = serde_json::to_value(&features).unwrap();
        assert!(super::projection::project(
            scan,
            &native.design_types,
            &native.design_record_headers,
            &mut features,
            &inputs
        )
        .unwrap()
        .is_empty());
        assert_eq!(serde_json::to_value(&features).unwrap(), first);
        (
            features.remove(0),
            losses.into_iter().map(|loss| loss.code).collect(),
        )
    })
}

#[test]
fn pipe_projection_retains_square_triangle_and_unknown_section_values() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    for (stored, label) in [(1, Some("square")), (2, Some("triangular")), (99, None)] {
        let (feature, losses) = projected_fixture(
            &Fixture::new(Config {
                values: [0.25, 0.75, 0.6, 0.15],
                section: Some(SectionFrame::of(stored)),
                ..Default::default()
            }),
            |_, _| {},
        );
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape:
                SweepShape::Solid {
                    op,
                    section: SweepSection::Unresolved(None),
                    ..
                },
            path: Some(PathRef::Native(_)),
            path_extent: Some(extent),
            ..
        }) = feature.evaluation.definition()
        else {
            panic!("partial Pipe Sweep")
        };
        assert_eq!(*op, cadmpeg_ir::features::SolidSweepOperation::NewBody);
        assert_eq!(extent.along_fraction.get(), 0.25);
        assert_eq!(extent.against_fraction.get(), 0.75);
        assert_eq!(
            feature
                .source_properties
                .get("reference:0")
                .map(String::as_str),
            Some("100")
        );
        assert_eq!(
            feature
                .source_properties
                .get("pipe:section-size-cm")
                .map(String::as_str),
            Some("0.6")
        );
        assert_eq!(
            feature
                .source_properties
                .get("pipe:section-thickness-cm")
                .map(String::as_str),
            Some("0.15")
        );
        let source: serde_json::Value =
            serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
        assert_eq!(source["section_shape"].as_str(), label);
        assert_eq!(
            source["scopes"][0]["path_feature_construction"]["section"]["shape_value"],
            stored
        );
        assert_eq!(source["parameters"]["D"], "6 mm");
        assert_eq!(source["owners"].as_array().unwrap().len(), 4);
        assert_eq!(
            losses,
            [
                F3dLossCode::FeaturePipeSectionUnresolved.kind(),
                F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
                F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
                F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
            ]
        );
    }
}

#[test]
fn pipe_projection_preserves_inactive_thickness_and_out_of_range_fractions() {
    let (feature, losses) = projected_fixture(
        &crate::test_support::pipe::Fixture::new(crate::test_support::pipe::Config {
            values: [1.25, -0.25, 0.5, -1.0],
            hollow: false,
            ..Default::default()
        }),
        |_, _| {},
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                section: SweepSection::Unresolved(None),
                ..
            },
            path_extent: None,
            ..
        })
    ));
    for (key, value) in [
        ("pipe:forward-fraction", "1.25"),
        ("pipe:reverse-fraction", "-0.25"),
        ("pipe:section-thickness-cm", "-1"),
        ("pipe:is-hollow", "false"),
    ] {
        assert_eq!(
            feature.source_properties.get(key).map(String::as_str),
            Some(value)
        );
    }
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathExtentUnresolved.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_preserves_partial_point_data_and_prefix_controls() {
    use crate::test_support::pipe::{Config, Fixture, Layout};
    let (feature, losses) = projected_fixture(
        &Fixture::new(Config {
            values: [0.25, 0.75, 0.6, 0.15],
            point_version: Some(2),
            layout: Layout {
                extra_reference: true,
                ..Default::default()
            },
            ..Default::default()
        }),
        |native, _| {
            assert!(matches!(
                native.design_parameter_scopes[0].payload(),
                DesignScopePayload::Pipe(None)
            ));
            native
                .design_record_headers
                .retain(|header| header.record_index != 510);
        },
    );
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
    assert_eq!(source["observation"]["prefix"]["operation_value"], 4);
    assert_eq!(
        source["observation"]["points"][0]["point_record_index"],
        500
    );
    assert_eq!(
        source["observation"]["points"][0]["position"],
        serde_json::json!([0.25, 0.5, 0.75])
    );
    assert_eq!(
        source["observation"]["points"][0]["rule"]["reference_type"],
        1
    );
    assert_eq!(
        source["observation"]["points"][0]["rule"]["inputs"][0]["record_index"],
        510
    );
    assert!(source["observation"]["prefix_references"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!([500, 29])));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
            F3dLossCode::FeaturePipePointOriginUnresolved.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_keeps_unknown_operation_when_source_is_missing() {
    let (feature, losses) = projected_fixture(
        &crate::test_support::pipe::Fixture::new(Default::default()),
        |_, feature| feature.native_ref = None,
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            path: None,
            path_extent: None,
            ..
        })
    ));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
    assert_eq!(source["parameters"]["D"], "6 mm");
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeHollowUnresolved.kind(),
            F3dLossCode::FeaturePipeOperationUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathUnresolved.kind(),
            F3dLossCode::FeaturePipePathExtentUnresolved.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_keeps_operation_with_invalid_hollow_control() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    let mut fixture = Fixture::new(Config {
        values: [0.25, 0.75, 0.6, 0.15],
        section: Some(SectionFrame::of(1)),
        ..Default::default()
    });
    fixture.bulk[21] = 2;
    let (feature, losses) = projected_fixture(&fixture, |native, _| {
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    });
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
                ..
            },
            ..
        })
    ));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
    assert_eq!(source["observation"]["prefix"]["is_hollow_value"], 2);
    assert_eq!(source["section_shape"], "square");
    assert_eq!(source["observation"]["section"]["shape_value"], 1);
    assert_eq!(source["observation"]["section"]["shape_offset"], 95);
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeHollowUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_uses_explicit_circle_with_partial_extent() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    for (hollow, thickness, wall, retained) in
        [(true, 0.1, Some(1.0), "0.1"), (false, -1.0, None, "-1")]
    {
        let (feature, losses) = projected_fixture(
            &Fixture::new(Config {
                hollow,
                values: [0.25, 0.75, 0.5, thickness],
                section: Some(SectionFrame::of(0)),
                ..Default::default()
            }),
            |_, _| {},
        );
        assert_eq!(
            circular_section(feature.evaluation.definition()),
            Some((2.5, wall))
        );
        assert_eq!(
            feature
                .source_properties
                .get("pipe:section-thickness-cm")
                .map(String::as_str),
            Some(retained)
        );
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            path_extent: Some(extent),
            ..
        }) = feature.evaluation.definition()
        else {
            panic!("directed circular Sweep")
        };
        assert_eq!(extent.along_fraction.get(), 0.25);
        assert_eq!(extent.against_fraction.get(), 0.75);
        assert_eq!(
            losses,
            [
                F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
                F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
                F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
            ]
        );
    }
}

#[test]
fn pipe_projection_reads_unit_fractions_without_a_point_selection_as_the_whole_path() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    use cadmpeg_ir::features::SolidSweepOperation;
    let (feature, losses) = projected_fixture(
        &Fixture::new(Config {
            values: [1.0, 1.0, 0.5, 0.1],
            section: Some(SectionFrame::of(0)),
            ..Default::default()
        }),
        |_, _| {},
    );
    assert_eq!(
        sweep_operation_and_extent(feature.evaluation.definition()),
        Some((SolidSweepOperation::NewBody, 1.0, 1.0))
    );
    assert!(losses.is_empty());
}

#[test]
fn pipe_projection_settles_unit_fractions_of_another_section_without_a_point_selection() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    let parameterization = F3dLossCode::FeaturePipePathParameterizationAssumed.kind();
    let (_, square) = projected_fixture(
        &Fixture::new(Config {
            values: [1.0, 1.0, 0.5, 0.1],
            section: Some(SectionFrame::of(1)),
            ..Default::default()
        }),
        |_, _| {},
    );
    assert!(!square.contains(&parameterization), "{square:?}");
    let (_, point_selected) = projected_fixture(
        &Fixture::new(Config {
            values: [1.0, 1.0, 0.5, 0.1],
            point_version: Some(2),
            ..Default::default()
        }),
        |_, _| {},
    );
    assert!(
        point_selected.contains(&parameterization),
        "{point_selected:?}"
    );
}

#[test]
fn pipe_projection_keeps_existing_explicit_circle_output() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    let (feature, losses) = projected_fixture(
        &Fixture::new(Config {
            values: [1.0, 0.0, 0.5, 0.1],
            section: Some(SectionFrame::of(0)),
            ..Default::default()
        }),
        |_, _| {},
    );
    assert_eq!(
        circular_section(feature.evaluation.definition()),
        Some((2.5, None))
    );
    assert_eq!(feature.source_properties.len(), 1);
    assert_eq!(
        feature
            .source_properties
            .get("reference:0")
            .map(String::as_str),
        Some("100")
    );
    assert_eq!(
        losses,
        [F3dLossCode::FeaturePipePathEndSectionAssumed.kind()]
    );
}

#[test]
fn pipe_projection_missing_or_ambiguous_owner_keeps_other_scalar_roles() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    for duplicate in [false, true] {
        let (feature, losses) = projected_fixture(
            &Fixture::new(Config {
                values: [0.25, 0.75, 0.6, 0.15],
                section: Some(SectionFrame::of(1)),
                ..Default::default()
            }),
            |native, _| {
                if duplicate {
                    let owner = native
                        .design_parameter_owners
                        .iter()
                        .find(|owner| owner.record_index() == 103)
                        .unwrap()
                        .clone();
                    native.design_parameter_owners.push(owner);
                } else {
                    native
                        .design_parameter_owners
                        .retain(|owner| owner.record_index() != 103);
                }
            },
        );
        assert_eq!(
            feature
                .source_properties
                .get("pipe:forward-fraction")
                .map(String::as_str),
            Some("0.25")
        );
        assert!(!feature
            .source_properties
            .contains_key("pipe:reverse-fraction"));
        assert_eq!(
            feature
                .source_properties
                .get("pipe:section-size-cm")
                .map(String::as_str),
            Some("0.6")
        );
        assert_eq!(
            feature
                .source_properties
                .get("pipe:section-thickness-cm")
                .map(String::as_str),
            Some("0.15")
        );
        assert_eq!(
            losses,
            [
                F3dLossCode::FeaturePipeScalarUnresolved.kind(),
                F3dLossCode::FeaturePipeSectionUnresolved.kind(),
                F3dLossCode::FeaturePipePathExtentUnresolved.kind(),
                F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
                F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
            ]
        );
    }
}

#[test]
fn pipe_projection_keeps_cut_when_target_group_is_missing() {
    use crate::test_support::pipe::{Config, Fixture, Layout};
    let (feature, losses) = projected_fixture(
        &Fixture::new(Config {
            operation: 2,
            values: [0.25, 0.75, 0.6, 0.15],
            layout: Layout {
                extra_reference: true,
                ..Default::default()
            },
            ..Default::default()
        }),
        |native, _| {
            native
                .design_construction_operand_groups
                .retain(|group| group.role() == DesignOperandRole::ROLE_0X5);
        },
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::Cut,
                ..
            },
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeTargetsUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_keeps_intersect_and_names_generic_path_assumption() {
    use crate::test_support::pipe::{Config, Fixture, SectionFrame};
    let (feature, losses) = projected_fixture(
        &Fixture::new(Config {
            operation: 3,
            values: [0.25, 0.75, 0.6, 0.15],
            section: Some(SectionFrame::of(1)),
            ..Default::default()
        }),
        |_, _| {},
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::Intersect,
                ..
            },
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeTargetsUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathRoleAssumed.kind(),
            F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_retains_ambiguous_scope_candidates_without_selecting_them() {
    let (feature, losses) = projected_fixture(
        &crate::test_support::pipe::Fixture::new(Default::default()),
        |native, _| {
            native
                .design_parameter_scopes
                .push(native.design_parameter_scopes[0].clone());
        },
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            path: None,
            path_extent: None,
            ..
        })
    ));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
    assert_eq!(source["scopes"].as_array().unwrap().len(), 2);
    assert_eq!(source["owners"].as_array().unwrap().len(), 4);
    assert_eq!(source["groups"].as_array().unwrap().len(), 1);
    assert_eq!(source["parameters"]["D"], "6 mm");
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeScalarUnresolved.kind(),
            F3dLossCode::FeaturePipeHollowUnresolved.kind(),
            F3dLossCode::FeaturePipeOperationUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathUnresolved.kind(),
            F3dLossCode::FeaturePipePathExtentUnresolved.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

#[test]
fn pipe_projection_keeps_raw_unknown_operation_and_decoded_controls() {
    use crate::test_support::pipe::{Config, Fixture};
    let mut fixture = Fixture::new(Config {
        values: [0.25, 0.75, 0.6, 0.15],
        ..Default::default()
    });
    fixture.bulk[22..26].copy_from_slice(&99_u32.to_le_bytes());
    let (feature, losses) = projected_fixture(&fixture, |_, _| {});
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("pipe:source").unwrap()).unwrap();
    assert_eq!(source["observation"]["prefix"]["operation_value"], 99);
    assert_eq!(
        feature
            .source_properties
            .get("pipe:is-hollow")
            .map(String::as_str),
        Some("false")
    );
    assert_eq!(
        feature
            .source_properties
            .get("pipe:section-size-cm")
            .map(String::as_str),
        Some("0.6")
    );
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeSourceUnresolved.kind(),
            F3dLossCode::FeaturePipeOperationUnresolved.kind(),
            F3dLossCode::FeaturePipeSectionUnresolved.kind(),
            F3dLossCode::FeaturePipePathRoleAssumed.kind(),
            F3dLossCode::FeaturePipePathParameterizationAssumed.kind(),
            F3dLossCode::FeaturePipePathTangentAssumedFalse.kind(),
            F3dLossCode::FeaturePipeLinearizeAssumedFalse.kind(),
        ]
    );
}

fn pipe_scope(operation: DesignExtrudeOperation, is_hollow: bool) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#20"),
        DesignFeatureKind::Pipe,
        20,
    );
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Pipe(Some(DesignPipeConstruction {
                operation,
                operation_offset: 22,
                is_hollow,
                is_hollow_offset: 21,
                opaque_post_operation: cadmpeg_ir::native::bytes::NativeBytes::new([0, 0]),
                opaque_post_operation_offset: 26,
                record_indexes: [21, 22, 23, 24],
                point_selection: None,
                section: None,
            }));
            draft.reference_count_offset = 40;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 96;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

/// The forward and reverse fractions 1, `size` and `thickness`, in
/// centimetres.
fn owners(size: f64, thickness: f64) -> Vec<DesignParameterOwner> {
    owners_with_fractions([1.0, 1.0], size, thickness)
}

fn owners_with_fractions(
    [forward, reverse]: [f64; 2],
    size: f64,
    thickness: f64,
) -> Vec<DesignParameterOwner> {
    [forward, reverse, size, thickness]
        .into_iter()
        .zip(21u32..)
        .map(|(value, record_index)| {
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("{STREAM}:design-parameter-owner#{record_index}"),
                byte_offset: 120,
                frame_length: 104,
                class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
                record_index,
                scope_record_index: 20,
                local_ordinal: record_index - 21,
                evaluated_value: value,
                evaluated_value_offset: 160,
                parameter_record_index: record_index - 1,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: record_index + 1,
            })
            .unwrap()
        })
        .collect()
}

fn path_group(record_index: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: 20,
        scope_reference_ordinal: 0,
        record_index,
        byte_offset: 900,
        class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
        members: vec![crate::records::identity::Located {
            value: record_index + 1,
            offset: 926,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 921,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 971,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 975,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 953,
        paired_class_tag: DesignClassTag::try_from("265".to_owned()).unwrap(),
        paired_byte_offset: 1024,
    })
    .unwrap()
}

fn cylinder(radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .unwrap(),
    ))
}

fn torus(minor_radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            20.0,
            minor_radius,
        )
        .unwrap(),
    ))
}

fn cone(radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
            1.0,
            0.3,
        )
        .unwrap(),
    ))
}

/// The unit cube with its first face carriers replaced by `carriers`.
fn result_model(carriers: &[SurfaceGeometry]) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for (surface, geometry) in ir.model.surfaces.iter_mut().zip(carriers) {
        surface.geometry = geometry.clone();
    }
    ir
}

fn pipe_feature(scope: &DesignParameterScope, ir: &cadmpeg_ir::CadIr) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#pipe").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Pipe".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Pipe".into(),
                parameters: Default::default(),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()])
                .unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

/// The definition the binder gives a Pipe over `scope`, `owners`, `groups`
/// and the cube result whose first carriers are `carriers`.
fn bound(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
) -> FeatureDefinition {
    bound_with(scope, owners, groups, carriers, |_, _| {})
}

/// `bound`, after `edit` changes the result model and the feature list.
fn bound_with(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
    edit: impl FnOnce(&mut cadmpeg_ir::CadIr, &mut Vec<Feature>),
) -> FeatureDefinition {
    bound_with_losses(scope, owners, groups, carriers, edit).0
}

/// `bound_with`, and the loss kinds the binder reports.
fn bound_with_losses(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
    edit: impl FnOnce(&mut cadmpeg_ir::CadIr, &mut Vec<Feature>),
) -> (FeatureDefinition, Vec<LossKind>) {
    let mut ir = result_model(carriers);
    let mut features = vec![pipe_feature(scope, &ir)];
    edit(&mut ir, &mut features);
    let losses = bind_circular_pipe_sections(
        &mut features,
        &PipeSectionInputs {
            scopes: std::slice::from_ref(scope),
            groups,
            owners,
            bodies: &ir.model.bodies,
            regions: &ir.model.regions,
            faces: &ir.model.faces,
            surfaces: &ir.model.surfaces,
            linear_tolerance: LINEAR_TOLERANCE,
        },
    );
    let definition = features[0].evaluation.definition().clone();
    let losses = losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>();
    let projected = matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
    );
    let read_circle = matches!(
        scope.payload(),
        DesignScopePayload::Pipe(Some(DesignPipeConstruction {
            section: Some(DesignPipeSection { shape_value: 0, .. }),
            ..
        }))
    );
    let count = |codes: [F3dLossCode; 2]| {
        losses
            .iter()
            .filter(|loss| codes.iter().any(|code| code.kind() == **loss))
            .count()
    };
    let section = count([
        F3dLossCode::FeaturePipeCircularSectionInferred,
        F3dLossCode::FeaturePipeCircularSectionAssumed,
    ]);
    let extent = count([
        F3dLossCode::FeaturePipeWholePathExtentAssumed,
        F3dLossCode::FeaturePipePathEndSectionAssumed,
    ]);
    let fraction = |ordinal: u32| {
        owners
            .iter()
            .find(|owner| owner.local_ordinal() == ordinal)
            .map(|owner| owner.evaluated_value().get())
    };
    let point_selected = matches!(
        scope.payload(),
        DesignScopePayload::Pipe(Some(DesignPipeConstruction {
            point_selection: Some(_),
            ..
        }))
    );
    let open_path = !point_selected && fraction(0) == Some(1.0) && fraction(1) == Some(1.0);
    assert_eq!(section, usize::from(projected && !read_circle));
    assert_eq!(extent, usize::from(projected && !open_path));
    assert_eq!(losses.len(), section + extent);
    (definition, losses)
}

/// The outer radius and wall thickness of a bound circular section.
fn circular_section(definition: &FeatureDefinition) -> Option<(f64, Option<f64>)> {
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape:
            SweepShape::Solid {
                section: SweepSection::Generated(GeneratedSweepSection::CircularRegion { region }),
                ..
            },
        ..
    }) = definition
    else {
        return None;
    };
    Some((
        region.outer_radius().get(),
        region
            .wall_thickness()
            .map(cadmpeg_ir::scalar::PositiveLength::get),
    ))
}

#[test]
fn a_new_body_pipe_with_circular_result_faces_projects_as_a_circular_sweep() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let (definition, losses) = bound_with_losses(
        &scope,
        &owners(0.5, 0.1),
        &groups,
        &[cylinder(2.5)],
        |_, _| {},
    );
    assert_eq!(circular_section(&definition), Some((2.5, None)));
    assert_eq!(
        losses,
        [F3dLossCode::FeaturePipeCircularSectionInferred.kind()]
    );
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        path: Some(PathRef::Native(path)),
        path_extent: Some(extent),
        ..
    }) = &definition
    else {
        panic!("Sweep along the path group: {definition:?}");
    };
    assert_eq!(path, &groups[0].id);
    assert_eq!(extent.along_fraction.get(), 1.0);
    assert_eq!(extent.against_fraction.get(), 1.0);
    for carrier in [torus(2.5), torus(-2.5)] {
        assert_eq!(
            circular_section(&bound(&scope, &owners(0.5, 0.1), &groups, &[carrier])),
            Some((2.5, None))
        );
    }

    let hollow = pipe_scope(DesignExtrudeOperation::NewBody, true);
    let definition = bound(
        &hollow,
        &owners(0.5, 0.1),
        &groups,
        &[cylinder(2.5), cylinder(1.5)],
    );
    assert_eq!(circular_section(&definition), Some((2.5, Some(1.0))));
}

#[test]
fn a_pipe_needs_curved_result_faces_of_its_section_radii() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let hollow = pipe_scope(DesignExtrudeOperation::NewBody, true);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    for (scope, carriers) in [
        (&scope, Vec::new()),
        (&scope, vec![cylinder(3.0)]),
        (&scope, vec![cylinder(2.5), cylinder(1.5)]),
        (&scope, vec![cylinder(2.5), cone(2.5)]),
        (&hollow, vec![cylinder(2.5)]),
    ] {
        assert!(
            matches!(
                bound(scope, &owners(0.5, 0.1), &groups, &carriers),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ),
            "{carriers:?}"
        );
    }
}

#[test]
fn a_pipe_needs_one_path_group_and_its_scalar_owners() {
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let mut missing_owner = owners(0.5, 0.1);
    missing_owner.pop();
    let mut duplicate_owner = owners(0.5, 0.1);
    duplicate_owner.extend(owners(0.5, 0.1).pop());
    let two_paths = [
        path_group(30, DesignOperandRole::ROLE_0X5),
        path_group(40, DesignOperandRole::ROLE_0X5),
    ];
    let no_path = [path_group(30, DesignOperandRole::BODIES_A)];
    let new_body = pipe_scope(DesignExtrudeOperation::NewBody, false);
    for (scope, owners, groups) in [
        (&new_body, missing_owner, &groups[..]),
        (&new_body, duplicate_owner, &groups[..]),
        (&new_body, owners(0.5, 0.1), &two_paths[..]),
        (&new_body, owners(0.5, 0.1), &no_path[..]),
    ] {
        assert!(matches!(
            bound(scope, &owners, groups, &[cylinder(2.5)]),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
    }
}

#[test]
fn a_pipe_needs_whole_path_fractions() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    for fractions in [[0.5, 1.0], [1.0, 0.5], [0.0, 0.0]] {
        let (definition, losses) = bound_with_losses(
            &scope,
            &owners_with_fractions(fractions, 0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |_, _| {},
        );
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert!(losses.is_empty());
    }
}

#[test]
fn a_pipe_whose_results_a_later_feature_changes_assumes_a_circle_and_unchanged_results_must_fit() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let later = |features: &mut Vec<Feature>, definition: FeatureDefinition, shares: bool| {
        let mut feature = features[0].clone();
        feature.id = FeatureId::mint("test:model:feature#later").unwrap();
        feature.ordinal = 1;
        feature.native_ref = None;
        let outputs = if shares {
            features[0].evaluation.outputs().clone()
        } else {
            Vec::new()
        };
        let outputs = cadmpeg_ir::features::DistinctMembers::try_from(outputs).unwrap();
        feature.evaluation = FeatureEvaluation::new(definition, outputs);
        features.push(feature);
    };
    let native = || {
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Hole".into(),
            parameters: Default::default(),
        })
    };
    for shares in [true, false] {
        let (definition, losses) = bound_with_losses(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(4.0)],
            |_, features| later(features, native(), shares),
        );
        assert_eq!(circular_section(&definition), Some((2.5, None)));
        assert_eq!(
            losses,
            [F3dLossCode::FeaturePipeCircularSectionAssumed.kind()]
        );
    }
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| ir.model.regions.clear()
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| {
                let mut twin = ir.model.surfaces[0].clone();
                twin.geometry = cylinder(4.0);
                ir.model.surfaces.push(twin);
            }
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

/// The operation, along fraction and against fraction of a bound Sweep.
fn sweep_operation_and_extent(
    definition: &FeatureDefinition,
) -> Option<(cadmpeg_ir::features::SolidSweepOperation, f64, f64)> {
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: SweepShape::Solid { op, .. },
        path_extent: Some(extent),
        ..
    }) = definition
    else {
        return None;
    };
    Some((
        *op,
        extent.along_fraction.get(),
        extent.against_fraction.get(),
    ))
}

#[test]
fn a_pipe_with_one_unit_and_one_zero_fraction_projects_with_a_path_end_section() {
    use cadmpeg_ir::features::SolidSweepOperation;
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    for [forward, reverse] in [[1.0, 0.0], [0.0, 1.0]] {
        let (definition, losses) = bound_with_losses(
            &scope,
            &owners_with_fractions([forward, reverse], 0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |_, _| {},
        );
        assert_eq!(circular_section(&definition), Some((2.5, None)));
        assert_eq!(
            sweep_operation_and_extent(&definition),
            Some((SolidSweepOperation::NewBody, forward, reverse))
        );
        assert_eq!(
            losses,
            [
                F3dLossCode::FeaturePipeCircularSectionInferred.kind(),
                F3dLossCode::FeaturePipePathEndSectionAssumed.kind()
            ]
        );
    }
}

#[test]
fn a_boolean_pipe_projects_its_operation_with_an_assumed_circle() {
    use cadmpeg_ir::features::SolidSweepOperation;
    let groups = [
        path_group(30, DesignOperandRole::BODIES_A),
        path_group(40, DesignOperandRole::ROLE_0X5),
    ];
    for (operation, op) in [
        (DesignExtrudeOperation::Join, SolidSweepOperation::Join),
        (DesignExtrudeOperation::Cut, SolidSweepOperation::Cut),
    ] {
        let scope = pipe_scope(operation, false);
        let (definition, losses) = bound_with_losses(
            &scope,
            &owners_with_fractions([1.0, 0.0], 3.2, 0.8),
            &groups,
            &[cylinder(4.0)],
            |_, _| {},
        );
        assert_eq!(circular_section(&definition), Some((16.0, None)));
        assert_eq!(
            sweep_operation_and_extent(&definition),
            Some((op, 1.0, 0.0))
        );
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        }) = &definition
        else {
            panic!("Sweep along the path group: {definition:?}");
        };
        assert_eq!(path, &groups[1].id);
        assert_eq!(
            losses,
            [
                F3dLossCode::FeaturePipeCircularSectionAssumed.kind(),
                F3dLossCode::FeaturePipePathEndSectionAssumed.kind()
            ]
        );
    }
    assert!(matches!(
        bound_with(
            &pipe_scope(DesignExtrudeOperation::Intersect, false),
            &owners_with_fractions([1.0, 0.0], 3.2, 0.8),
            &groups,
            &[cylinder(4.0)],
            |_, _| {},
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

#[test]
fn a_new_body_pipe_without_result_bodies_assumes_a_circle() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, true);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let (definition, losses) =
        bound_with_losses(&scope, &owners(0.6, 0.15), &groups, &[], |_, features| {
            features[0].evaluation = FeatureEvaluation::new(
                features[0].evaluation.definition().clone(),
                cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
            );
        });
    assert_eq!(circular_section(&definition), Some((3.0, Some(1.5))));
    assert_eq!(
        losses,
        [F3dLossCode::FeaturePipeCircularSectionAssumed.kind()]
    );
}

#[test]
fn a_point_selected_pipe_with_unit_fractions_assumes_its_whole_path_extent() {
    use crate::test_support::pipe::{Config, Fixture};
    let ctx = cadmpeg_test_support::service_decode_context();
    let native = Fixture::new(Config {
        values: [1.0, 1.0, 0.5, 0.1],
        point_version: Some(2),
        ..Default::default()
    })
    .decode(&ctx)
    .unwrap();
    let scope = &native.design_parameter_scopes[0];
    assert!(matches!(
        scope.payload(),
        DesignScopePayload::Pipe(Some(DesignPipeConstruction {
            point_selection: Some(_),
            ..
        }))
    ));
    let (definition, losses) = bound_with_losses(
        scope,
        &native.design_parameter_owners,
        &native.design_construction_operand_groups,
        &[cylinder(2.5)],
        |_, _| {},
    );
    assert_eq!(circular_section(&definition), Some((2.5, None)));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeCircularSectionInferred.kind(),
            F3dLossCode::FeaturePipeWholePathExtentAssumed.kind()
        ]
    );
}

/// `pipe_scope` whose section frame stores `shape`.
fn shaped_pipe_scope(operation: DesignExtrudeOperation, shape_value: u32) -> DesignParameterScope {
    let mut scope = pipe_scope(operation, false);
    scope
        .try_edit(|draft| {
            if let DesignScopePayload::Pipe(Some(construction)) = &mut draft.payload {
                construction.section = Some(DesignPipeSection {
                    shape_value,
                    shape_offset: 95,
                });
            }
            draft.reference_count_offset = 99;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 96;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn a_read_circular_shape_projects_without_a_section_test() {
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let (definition, losses) = bound_with_losses(
        &shaped_pipe_scope(DesignExtrudeOperation::NewBody, 0),
        &owners(0.5, 0.1),
        &groups,
        &[cone(2.5)],
        |_, _| {},
    );
    assert_eq!(circular_section(&definition), Some((2.5, None)));
    assert!(losses.is_empty());
}

#[test]
fn a_read_shape_other_than_a_circle_keeps_the_pipe_native() {
    let groups = [
        path_group(30, DesignOperandRole::BODIES_A),
        path_group(40, DesignOperandRole::ROLE_0X5),
    ];
    for shape in [1, 2, 3] {
        for operation in [
            DesignExtrudeOperation::NewBody,
            DesignExtrudeOperation::Join,
        ] {
            let groups = if operation == DesignExtrudeOperation::Join {
                &groups[..]
            } else {
                &groups[1..]
            };
            assert!(matches!(
                bound(
                    &shaped_pipe_scope(operation, shape),
                    &owners(0.5, 0.1),
                    groups,
                    &[cylinder(2.5)],
                ),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
        }
    }
}
