// SPDX-License-Identifier: Apache-2.0

use super::{
    neutral_parameter, raw_parameter, reference, reference_list, test_feature, test_label,
    test_projection_index, test_property,
};
use crate::feature::{
    project_chamfer, FeatureInventory, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectedSketches, CHAMFER_CLASS_ID,
};
use crate::pmdc::PmDcU32List;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    edge_treatments::{ChamferForm, ChamferSpec},
    DesignParameter, FeatureDefinition, FeatureOperation, ParameterValue,
};
use cadmpeg_ir::scalar::{Angle, Length};

fn index_list(values: &[u32]) -> PmDcU32List {
    PmDcU32List::new(
        2,
        (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
        values.to_vec(),
    )
    .expect("index list")
}

fn chamfer_properties(form: u16) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: index_list(&[1]),
                index_reference_value: -1,
                value: 0,
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[5]),
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::FaceItem {
                index_references: index_list(&[2]),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Chamfer,
                type_value: 2,
                value: form,
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[9]),
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ]
}

fn chamfer_source(ordinal: u32) -> PmDcFeature {
    test_feature(
        ordinal,
        12,
        &[
            (0, 1),
            (1, 3),
            (2, 40),
            (3, 41),
            (4, 5),
            (5, 6),
            (10, 42),
            (11, 7),
        ],
    )
}

fn dimensions(
    raw: &[crate::design::PmDcParameter; 3],
    second: f64,
    angle: f64,
) -> Vec<DesignParameter> {
    vec![
        neutral_parameter(
            &raw[0],
            ParameterValue::Length(Length::new(2.0).expect("length")),
        ),
        neutral_parameter(
            &raw[1],
            ParameterValue::Length(Length::new(second).expect("length")),
        ),
        neutral_parameter(
            &raw[2],
            ParameterValue::Angle(Angle::new(angle).expect("angle")),
        ),
    ]
}

#[test]
fn oriented_chamfers_keep_support_identity_and_require_admitted_dimensions() {
    let raw = [raw_parameter(40), raw_parameter(41), raw_parameter(42)];
    let source = chamfer_source(100);
    let label = test_label(100, 1, CHAMFER_CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    for (form, second, angle, admitted) in [
        (1, 3.0, 0.5, true),
        (2, 3.0, 0.5, true),
        (2, 2.0, 0.5, true),
        (2, 2.0 + f64::EPSILON * 2.0, 0.5, true),
        (1, 3.0, 0.0, false),
        (1, 3.0, std::f64::consts::PI, false),
        (2, 0.0, 0.5, false),
        (2, -1.0, 0.5, false),
        (3, 2.0, 0.5, false),
    ] {
        let properties = chamfer_properties(form);
        let neutral = dimensions(&raw, second, angle);
        let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
        let projected = project_chamfer(&ctx, &source, &label, &index);
        if !admitted {
            assert!(projected.is_none());
            continue;
        }
        let (projected, result) = projected.expect("chamfer candidate").expect("projection");
        let FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            flip_direction,
        }) = projected.evaluation.definition()
        else {
            panic!("chamfer operation")
        };
        assert!(*flip_direction);
        assert_eq!(
            groups[0].edges,
            cadmpeg_ir::features::EdgeSelection::Native(source.id())
        );
        if form == 1 {
            assert_eq!(
                groups[0].spec,
                ChamferSpec::Unresolved {
                    form: Some(ChamferForm::DistanceAngle),
                }
            );
        } else if second == 2.0 {
            let bound =
                |distance: &cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveLength>| {
                    distance
                        .value_from(&neutral)
                        .expect("bound chamfer distance")
                };
            assert!(matches!(
                &groups[0].spec,
                ChamferSpec::TwoDistances { first, second }
                    if bound(first) == cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive")
                        && bound(second) == cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive")
            ));
        } else {
            assert_eq!(
                groups[0].spec,
                ChamferSpec::Unresolved {
                    form: Some(ChamferForm::TwoDistances),
                }
            );
        }
        assert_eq!(result.bodies().len(), 1);
        assert_eq!(result.bodies()[0].as_str(), properties[7].id());
    }
    for slot in [0, 1, 2] {
        let properties = chamfer_properties(if slot == 2 { 1 } else { 2 });
        let mut neutral = dimensions(&raw, 2.0, 0.5);
        neutral[slot].value = Some(if slot == 2 {
            ParameterValue::Length(Length::new(0.5).expect("length"))
        } else {
            ParameterValue::Angle(Angle::new(2.0).expect("angle"))
        });
        let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
        assert!(project_chamfer(&ctx, &source, &label, &index).is_none());
    }
}

#[test]
fn oriented_chamfers_require_nonempty_resolved_support_items() {
    let raw = [raw_parameter(40), raw_parameter(41), raw_parameter(42)];
    let neutral = dimensions(&raw, 2.0, 0.5);
    let source = chamfer_source(100);
    let label = test_label(100, 1, CHAMFER_CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let properties = chamfer_properties(2);
    let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
    assert!(project_chamfer(&ctx, &source, &label, &index)
        .expect("valid support")
        .is_ok());
    for members in [&[][..], &[0][..], &[3][..], &[99][..]] {
        let mut properties = chamfer_properties(2);
        properties[2] = test_property(
            3,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(members),
            },
        );
        let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
        assert!(project_chamfer(&ctx, &source, &label, &index).is_none());
    }
    let mut properties = chamfer_properties(2);
    properties[3] = test_property(
        4,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: index_list(&[]),
        },
    );
    let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &[], &[]);
    assert!(project_chamfer(&ctx, &source, &label, &index).is_none());
}

#[test]
fn unresolved_chamfer_dimensions_remain_in_loss_count_after_ordinal_filtering() {
    let raw = [raw_parameter(40), raw_parameter(41), raw_parameter(42)];
    let design = crate::design::DesignInventory {
        parameters: raw.to_vec(),
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: Vec::new(),
        brep_segments: Vec::new(),
        issues: Vec::new(),
    };
    let sketch = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    for (form, second, duplicate, expected_count, expected_features) in [
        (1, 2.0, false, 1, 1),
        (2, 3.0, false, 1, 1),
        (2, 2.0, false, 0, 1),
        (1, 2.0, true, 2, 0),
    ] {
        let mut inventory = FeatureInventory {
            record_types: std::collections::HashMap::new(),
            features: vec![chamfer_source(100)],
            pattern_features: Vec::new(),
            terminators: Vec::new(),
            properties: chamfer_properties(form),
            dimensions: Vec::new(),
            labels: vec![test_label(100, 1, CHAMFER_CLASS_ID, &[])],
            entity_style_links: Vec::new(),
            kernel_entity_links: Vec::new(),
            work_planes: Vec::new(),
            issues: Vec::new(),
            edge_inputs: crate::feature::edge_selection::EdgeInventory::default(),
            ..Default::default()
        };
        if duplicate {
            inventory.features.push(chamfer_source(101));
            inventory
                .labels
                .push(test_label(101, 1, CHAMFER_CLASS_ID, &[]));
        }
        let neutral = dimensions(&raw, second, 0.5);
        let projected = crate::feature::project(
            &ctx,
            &inventory,
            &design,
            &sketch,
            &neutral,
            &ProjectedSketches {
                sketches: &[],
                entities: &[],
                profile_bodies: &[],
                result_model: None,
            },
        )
        .expect("feature projection");
        assert_eq!(projected.unresolved_features, expected_count);
        assert_eq!(projected.features.len(), expected_features);
        assert_eq!(projected.result_topologies.len(), expected_features);
    }
}
