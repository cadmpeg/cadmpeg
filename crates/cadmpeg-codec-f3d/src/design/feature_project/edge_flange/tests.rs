use super::*;
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::records::feature::sheet_metal::edge::{
    DesignEdgeFlangeScope, DesignReferenceEdgeFlange, DesignReferenceFlangeEdge,
};
use crate::records::identity::Located;
use crate::records::parameters::{DesignParameter, DesignParameterDraft, DesignParameterSource};
use crate::test_support::with_decode_context;

fn parameter(index: u32, owner: u32, kind: &str, value: f64) -> DesignParameter {
    DesignParameter::try_from(DesignParameterDraft {
        id: format!("f3d:test/Design1/BulkStream.dat:parameter#{index}"),
        byte_offset: 0,
        class_tag: "000".to_owned().try_into().expect("class text"),
        record_index: index,
        source_ordinal: 0,
        source: DesignParameterSource::new(kind.to_owned(), Some(owner), None)
            .expect("owned parameter"),
        expression: "0".to_owned(),
        expression_offset: 20,
        source_kind_offset: 30,
        unit: Some(crate::records::identity::RecordedValue {
            value: if kind == "FlangeAngle" { "rad" } else { "mm" }.to_owned(),
            offset: 50,
        }),
        name: "p".to_owned(),
        name_offset: 60,
        evaluated_value: value,
        evaluated_value_offset: 80,
    })
    .expect("parameter fields")
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:scope#10",
        DesignFeatureKind::EdgeFlange,
        10,
    );
    crate::design::test_support::name_scope_references(&mut scope, [20, 30, 40, 50, 60, 70]);
    scope
}

#[test]
fn reference_edge_parameter_cache_units_and_conflicts_are_independent() {
    use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
    let source = scope();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:test/Design1/BulkStream.dat:owner#20".to_owned(),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "000".to_owned().try_into().expect("class text"),
        record_index: 20,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 2.5,
        evaluated_value_offset: 40,
        parameter_record_index: 21,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    })
    .expect("cached owner");
    let owners = [owner];
    let mut losses = Vec::new();
    assert_eq!(
        scalar(
            &source,
            Some(20),
            "FlangeHeight",
            &ProjectInputs {
                owners: &owners,
                ..Default::default()
            },
            &mut losses
        ),
        Some(25.0)
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::EdgeCachedParameterAssumed.kind()));
    let wrong = [parameter(21, 20, "FlangeAngle", 1.0)];
    losses.clear();
    assert_eq!(
        scalar(
            &source,
            Some(20),
            "FlangeHeight",
            &ProjectInputs {
                native: &wrong,
                owners: &owners,
                ..Default::default()
            },
            &mut losses
        ),
        None
    );
    assert!(losses.is_empty());
    let mut native = [parameter(21, 20, "FlangeHeight", 2.5)];
    native[0]
        .try_set_unit_value("opaque".to_owned())
        .expect("unresolved unit");
    losses.clear();
    assert_eq!(
        scalar(
            &source,
            Some(20),
            "FlangeHeight",
            &ProjectInputs {
                native: &native,
                ..Default::default()
            },
            &mut losses
        ),
        Some(25.0)
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::EdgeParameterUnitAssumed.kind()));
    let ambiguous = [
        parameter(21, 20, "FlangeHeight", 2.5),
        parameter(22, 20, "FlangeHeight", 3.0),
    ];
    assert_eq!(
        scalar(
            &source,
            Some(20),
            "FlangeHeight",
            &ProjectInputs {
                native: &ambiguous,
                ..Default::default()
            },
            &mut losses
        ),
        None
    );
}

#[test]
fn reference_edge_legacy_fallback_retains_different_and_missing_widths() {
    use crate::records::feature::sheet_metal::{
        DesignEdgeFlangeEdge, DesignEdgeFlangeOperation, DesignEdgeFlangeSelection,
        DesignEdgeFlangeShape, DesignFlangeEdgeWidth,
    };
    with_decode_context(|ctx| {
        let mut source = scope();
        crate::design::test_support::name_scope_references(&mut source, [80, 90, 100, 103, 107]);
        let edges = [
            DesignEdgeFlangeEdge {
                wrapper: 20,
                group_record_index: 80_u32.try_into().expect("first group"),
                aggregate_operand_record_index: 103,
            },
            DesignEdgeFlangeEdge {
                wrapper: 30,
                group_record_index: 90_u32.try_into().expect("second group"),
                aggregate_operand_record_index: 107,
            },
        ];
        let operation = DesignEdgeFlangeOperation {
            height_owner_record_index: 60,
            angle_owner_record_index: 70,
            auxiliary_reference_record_indices: Vec::new(),
            settings_record_index: 50,
            bend_radius: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("radius control"),
            bend_radius_offset: 90,
            height_datum: DesignSheetMetalHeightDatum::OuterFaces,
            bend_position: DesignBendPosition::Inside,
            selection: DesignEdgeFlangeSelection::try_new(
                DesignEdgeFlangeShape::SymmetricPerEdge(vec![
                    DesignFlangeEdgeWidth {
                        edge: edges[0],
                        owners: 40,
                    },
                    DesignFlangeEdgeWidth {
                        edge: edges[1],
                        owners: 50,
                    },
                ]),
                100,
            )
            .expect("per-edge width owners"),
        };
        if let DesignScopePayloadMut::EdgeFlange(slot) = source.payload_mut() {
            *slot = DesignEdgeFlangeScope::Legacy(operation);
        }
        let parameters = [
            parameter(41, 40, "EdgeWidth", 1.0),
            parameter(51, 50, "EdgeWidth", 2.0),
        ];
        for first_present in [true, false] {
            let mut losses = Vec::new();
            let inputs = ProjectInputs {
                native: if first_present {
                    &parameters
                } else {
                    &parameters[1..]
                },
                ..Default::default()
            };
            let value = construction(
                project(&source, &inputs, ctx, &mut losses).expect("independent legacy widths"),
            );
            let SheetMetalFlangeWidth::SymmetricPerEdge { widths } =
                value.width.expect("per-edge law")
            else {
                panic!("per-edge law")
            };
            assert_eq!(
                widths[0].width.map(PositiveLength::get),
                first_present.then_some(10.0)
            );
            assert_eq!(widths[1].width.expect("second width").get(), 20.0);
            assert_eq!(value.bend_position, Some(SheetMetalBendPosition::Inside));
        }
        let mut operation = source
            .edge_flange_operation()
            .expect("legacy operation")
            .clone();
        operation.selection = DesignEdgeFlangeSelection::try_new(
            DesignEdgeFlangeShape::TwoSides {
                edges: edges.to_vec(),
                owners: [40, 50],
            },
            100,
        )
        .expect("two-sided law");
        if let DesignScopePayloadMut::EdgeFlange(slot) = source.payload_mut() {
            *slot = DesignEdgeFlangeScope::Legacy(operation);
        }
        let parameters = [parameter(51, 50, "EdgeWidth_2", 2.0)];
        let value = construction(
            project(
                &source,
                &ProjectInputs {
                    native: &parameters,
                    ..Default::default()
                },
                ctx,
                &mut Vec::new(),
            )
            .expect("partial side widths"),
        );
        let SheetMetalFlangeWidth::TwoSides(width) = value.width.expect("partial two-sided law")
        else {
            panic!("partial side law")
        };
        assert!(width.first.is_none());
        assert_eq!(width.second.expect("second side").get(), 20.0);
        let mut operation = source
            .edge_flange_operation()
            .expect("legacy operation")
            .clone();
        operation.selection = DesignEdgeFlangeSelection::try_new(
            DesignEdgeFlangeShape::FullEdge {
                edges: vec![edges[0]],
                height:
                    crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                        target_group_record_index: 110,
                        target_operand_record_index: 113,
                        offset_owner_record_index: 120,
                        reference_record_indices: [130, 140],
                    },
            },
            100,
        )
        .expect("to-object law");
        if let DesignScopePayloadMut::EdgeFlange(slot) = source.payload_mut() {
            *slot = DesignEdgeFlangeScope::Legacy(operation);
        }
        let mut losses = Vec::new();
        let value = construction(
            project(&source, &ProjectInputs::default(), ctx, &mut losses)
                .expect("independent target"),
        );
        let SheetMetalFlangeHeight::ToObject(height) = value.height.expect("partial target") else {
            panic!("partial target")
        };
        assert!(height.offset.is_none());
        assert!(matches!(
            height.target,
            Some(cadmpeg_ir::features::sheet_metal_forms::SheetMetalFlangeHeightTarget::Native(_))
        ));
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::EdgeHeightTargetScopeAssumed.kind()));
    });
}

fn construction(definition: FeatureDefinition) -> SheetMetalEdgeFlangeConstruction {
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange(construction)) =
        definition
    else {
        panic!("neutral edge flange")
    };
    construction
}

#[test]
fn reference_edge_missing_construction_stays_neutral() {
    with_decode_context(|ctx| {
        let mut losses = Vec::new();
        let value = construction(
            project(&scope(), &ProjectInputs::default(), ctx, &mut losses)
                .expect("partial projection"),
        );
        assert_eq!(value, SheetMetalEdgeFlangeConstruction::default());
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::EdgeReferenceEnvelopeUnresolved.kind()));
    });
}

#[test]
fn reference_edge_missing_frame_recovers_unique_scoped_parameter_roles() {
    with_decode_context(|ctx| {
        let source = scope();
        let parameters = [
            parameter(21, 20, "FlangeHeight", 2.5),
            parameter(31, 30, "FlangeAngle", -1.25),
            parameter(61, 60, "FlangeHeight", 3.0),
        ];
        for ambiguous_height in [false, true] {
            let inputs = ProjectInputs {
                native: if ambiguous_height {
                    &parameters
                } else {
                    &parameters[..2]
                },
                ..Default::default()
            };
            let mut losses = Vec::new();
            let value = construction(
                project(&source, &inputs, ctx, &mut losses)
                    .expect("partial frame with definitions"),
            );
            assert_eq!(value.height.is_none(), ambiguous_height);
            if !ambiguous_height {
                assert_eq!(
                    value.height,
                    Some(SheetMetalFlangeHeight::Distance(
                        PositiveLength::new(25.0).expect("height")
                    ))
                );
            }
            assert_eq!(value.angle.expect("independent angle").get(), -1.25);
            assert!(losses
                .iter()
                .any(|loss| loss.code == F3dLossCode::EdgeReferenceEnvelopeUnresolved.kind()));
            assert!(losses
                .iter()
                .any(|loss| loss.code == F3dLossCode::EdgeParameterOwnerBacklinkAssumed.kind()));
        }
    });
}

#[test]
fn reference_edge_independent_controls_signed_offsets_and_zero_radius_survive() {
    with_decode_context(|ctx| {
        let mut source = scope();
        let native = DesignReferenceEdgeFlange {
            bend_position: Some(Located {
                value: 2,
                offset: 21,
            }),
            height_datum: Some(Located {
                value: 2,
                offset: 60,
            }),
            height_owner: Some(Located {
                value: 20,
                offset: 64,
            }),
            angle_owner: Some(Located {
                value: 30,
                offset: 75,
            }),
            radius_control: Some(Located {
                value: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive control"),
                offset: 90,
            }),
            edges: vec![DesignReferenceFlangeEdge {
                group: Some(Located {
                    value: 70,
                    offset: 120,
                }),
                wrapper_member_count: Some(Located {
                    value: 5,
                    offset: 300,
                }),
                first_owner: Some(Located {
                    value: 40,
                    offset: 340,
                }),
                second_owner: Some(Located {
                    value: 50,
                    offset: 351,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        if let DesignScopePayloadMut::EdgeFlange(slot) = source.payload_mut() {
            *slot = DesignEdgeFlangeScope::ReferenceFramed(native);
        }
        let parameters = [
            parameter(21, 20, "FlangeHeight", 2.5),
            parameter(31, 30, "FlangeAngle", -1.25),
            parameter(41, 40, "EdgeOffset_1", 0.0),
            parameter(51, 50, "EdgeOffset_2", -0.75),
            parameter(61, 60, "BendRadius", 0.0),
        ];
        let inputs = ProjectInputs {
            native: &parameters,
            ..Default::default()
        };
        let mut losses = Vec::new();
        let value = construction(
            project(&source, &inputs, ctx, &mut losses).expect("independent projection"),
        );
        assert_eq!(
            value.height,
            Some(SheetMetalFlangeHeight::Distance(
                PositiveLength::new(25.0).expect("height")
            ))
        );
        assert_eq!(value.angle.expect("angle").get(), -1.25);
        assert_eq!(value.bend_radius.expect("zero radius").get(), 0.0);
        assert_eq!(value.bend_position, Some(SheetMetalBendPosition::Inside));
        let SheetMetalFlangeWidth::TwoOffsetsPerEdge { offsets } = value.width.expect("offset law")
        else {
            panic!("offset law")
        };
        assert_eq!(offsets.as_slice()[0].first.expect("zero offset").get(), 0.0);
        assert_eq!(
            offsets.as_slice()[0].second.expect("signed offset").get(),
            -7.5
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::EdgeParameterOwnerBacklinkAssumed.kind()));
        assert!(!losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::EdgeStoredRadiusInsideAssumed.kind()));
        let inputs = ProjectInputs {
            native: &parameters[1..4],
            ..Default::default()
        };
        let mut losses = Vec::new();
        let value = construction(
            project(&source, &inputs, ctx, &mut losses).expect("missing height definition"),
        );
        assert!(value.height.is_none());
        assert_eq!(value.angle.expect("independent angle").get(), -1.25);
        assert_eq!(value.bend_radius.expect("assumed radius").get(), 2.5);
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::EdgeStoredRadiusInsideAssumed.kind()));
    });
}
