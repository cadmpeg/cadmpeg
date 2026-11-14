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
fn reference_edge_independent_controls_signed_offsets_and_zero_radius_survive() {
    with_decode_context(|ctx| {
        let mut source = scope();
        let mut native = DesignReferenceEdgeFlange::default();
        native.bend_position = Some(Located {
            value: 2,
            offset: 21,
        });
        native.height_datum = Some(Located {
            value: 2,
            offset: 60,
        });
        native.height_owner = Some(Located {
            value: 20,
            offset: 64,
        });
        native.angle_owner = Some(Located {
            value: 30,
            offset: 75,
        });
        native.radius_control = Some(Located {
            value: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive control"),
            offset: 90,
        });
        native.edges = vec![DesignReferenceFlangeEdge {
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
        }];
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
