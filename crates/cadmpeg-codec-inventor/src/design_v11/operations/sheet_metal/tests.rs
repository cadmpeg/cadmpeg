// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::{DistinctMembers, EdgeSelection};
use cadmpeg_ir::features::sheet_metal_forms::SheetMetalCornerEffect;
use cadmpeg_ir::sketches::Sketch;
use serde_json::{json, Value};

fn sketch_data(sketches: &[Sketch]) -> SketchData<'_> {
    SketchData {
        sketches,
        ..SketchData::default()
    }
}

fn source(kind: &str, value: &Value) -> LegacyOperation {
    serde_json::from_value(json!({"kind": kind, "value": value}))
        .expect("synthetic sheet-metal operands")
}

#[test]
fn attached_plate_requires_a_separate_plate_contract() {
    let attachments = [
        json!({"kind":"native","value":"inventor:edges#attachment"}),
        json!({"kind":"historical","value":{
            "state":"inventor:design:feature-input#plate",
            "edges":["inventor:input-edge#second","inventor:input-edge#first"],
            "native":"inventor:edges#attachment"
        }}),
        json!({"kind":"generated","value":{
            "edges":[{"feature":"inventor:design:feature#base","local_id":"edge-2"},
                     {"feature":"inventor:design:feature#other","local_id":"edge-1"}],
            "native":"inventor:edges#attachment"
        }}),
    ];
    for attachment in attachments {
        for side in ["forward", "reverse", "symmetric"] {
            let value = source(
                "sheet_metal_attached_plate",
                &json!({
                    "profile":{"kind":"native","value":"inventor:profile#plate"},
                    "thickness":2.0,"side":side,"attachment":attachment
                }),
            );
            assert!(operation(&value, SketchData::default(), &[])
                .expect("sheet-metal family")
                .is_err());
        }
    }
}

#[test]
fn attached_bend_requires_plate_relief_and_transition_operands() {
    let value = source(
        "sheet_metal_bend",
        &json!({
            "plate":"inventor:design:feature#plate",
            "attachment":{"kind":"native","value":"inventor:edges#attachment"},
            "inside_radius":1.0,
            "relief":{"form":"style_default","width":2.0,"depth":3.0,"minimum_remnant":4.0,"seam_gap":0.5},
            "transition":{"form":"style_default","radius":6.0}
        }),
    );
    assert!(operation(&value, SketchData::default(), &[])
        .expect("sheet-metal family")
        .is_err());
}

#[test]
fn unchanged_corner_requires_a_geometry_preserving_contract() {
    let value = source(
        "sheet_metal_corner",
        &json!({
            "bend":"inventor:design:feature#bend","effect":"unchanged"
        }),
    );
    assert!(operation(&value, SketchData::default(), &[])
        .expect("sheet-metal family")
        .is_err());
}

fn feature(key: &str, value: LegacyOperation) -> old::Feature {
    old::Feature {
        id: old::FeatureId::mint(format!("inventor:design:feature#{key}"))
            .expect("feature identity"),
        product_definition: None,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: old::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(old::FeatureDefinition::Operation(value)),
        native_ref: None,
    }
}

#[test]
fn attached_history_preserves_order_states_and_distinct_lengths() {
    let sketch = Sketch {
        id: cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#plate")
            .expect("sketch identity"),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("sketch plane"),
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: cadmpeg_ir::sketches::SketchEntityId::mint("inventor:design:entity#circle")
                .expect("circle identity"),
            reversed: false,
        }]]
        .try_into()
        .expect("profile loop"),
        native_ref: None,
    };
    let attachment = json!({"kind":"historical","value":{
        "state":"inventor:design:feature-input#plate",
        "edges":["inventor:input-edge#second","inventor:input-edge#first"],
        "native":"inventor:edges#attachment"
    }});
    let plate = source(
        "sheet_metal_attached_plate",
        &json!({
            "profile":{"kind":"sketch","value":sketch.id},
            "thickness":2.0,"side":"reverse","attachment":attachment
        }),
    );
    let bend = source(
        "sheet_metal_bend",
        &json!({
            "plate":"inventor:design:feature#plate","attachment":attachment,"inside_radius":1.0,
            "relief":{"form":"style_default","width":2.0,"depth":3.0,"minimum_remnant":4.0,"seam_gap":0.5},
            "transition":{"form":"style_default","radius":6.0}
        }),
    );
    let features = [
        feature("plate", plate.clone()),
        feature("bend", bend.clone()),
    ];
    let sketches = [sketch];
    let FeatureOperation::SheetMetal(value) = operation(&plate, sketch_data(&sketches), &features)
        .expect("sheet-metal family")
        .expect("attached plate")
    else {
        panic!("sheet metal")
    };
    let SheetMetalKind::AttachedPlate {
        side, attachment, ..
    } = value.kind
    else {
        panic!("attached plate")
    };
    assert_eq!(side, NormalSide::Against);
    let cadmpeg_ir::design::selection::Picks::Named(items) = attachment.picks else {
        panic!("members")
    };
    let native = items
        .as_slice()
        .iter()
        .map(|item| {
            item.native
                .as_ref()
                .expect("historical edge name")
                .value
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        native,
        ["second", "first"].map(|edge| NativeNameValue::Text(
            cadmpeg_core::text::NonBlankString::new(format!(
                "[\"inventor:design:feature-input#plate\",\"inventor:input-edge#{edge}\"]"
            ))
            .expect("historical state and edge key")
        ))
    );
    let FeatureOperation::SheetMetal(value) = operation(&bend, sketch_data(&sketches), &features)
        .expect("sheet-metal family")
        .expect("attached bend")
    else {
        panic!("sheet metal")
    };
    let SheetMetalKind::AttachedBend {
        inside_radius,
        relief,
        transition_radius,
        ..
    } = value.kind
    else {
        panic!("bend")
    };
    assert_eq!(
        inside_radius,
        Dimension::Literal(PositiveLength::new(1.0).expect("positive radius"))
    );
    assert_eq!(
        relief.width,
        Dimension::Literal(PositiveLength::new(2.0).expect("positive width"))
    );
    assert_eq!(
        relief.depth,
        Dimension::Literal(PositiveLength::new(3.0).expect("positive depth"))
    );
    assert_eq!(
        relief.minimum_remnant,
        Dimension::Literal(
            cadmpeg_ir::scalar::NonNegativeLength::new(4.0).expect("nonnegative remnant")
        )
    );
    assert_eq!(
        relief.seam_gap,
        Dimension::Literal(
            cadmpeg_ir::scalar::NonNegativeLength::new(0.5).expect("nonnegative seam gap")
        )
    );
    assert_eq!(
        transition_radius,
        Dimension::Literal(
            cadmpeg_ir::scalar::NonNegativeLength::new(6.0).expect("nonnegative transition radius")
        )
    );
    let corner = source(
        "sheet_metal_corner",
        &json!({"bend":"inventor:design:feature#bend","effect":"unchanged"}),
    );
    assert!(
        matches!(operation(&corner, sketch_data(&sketches), &features), Some(Ok(FeatureOperation::SheetMetal(value))) if matches!(value.kind, SheetMetalKind::UnchangedCorner { .. }))
    );
}

#[test]
fn generated_attachments_keep_producer_and_local_name() {
    let producer = feature(
        "base",
        LegacyOperation::SheetMetalCorner {
            bend: old::FeatureId::mint("inventor:design:feature#bend").expect("bend identity"),
            effect: SheetMetalCornerEffect::Unchanged,
        },
    );
    let value: EdgeSelection = serde_json::from_value(json!({"kind":"generated","value":{
        "edges":[{"feature":producer.id,"local_id":"edge-2"},{"feature":producer.id,"local_id":"edge-1"}],
        "native":"inventor:edges#attachment"
    }})).expect("generated edge operands");
    assert!(attachments::edges(&value, &[]).is_err());
    let selection =
        attachments::edges(&value, std::slice::from_ref(&producer)).expect("generated attachment");
    let cadmpeg_ir::design::selection::Picks::Named(items) = selection.picks else {
        panic!("members")
    };
    for (item, name) in items.as_slice().iter().zip(["edge-2", "edge-1"]) {
        assert_eq!(
            item,
            &selections::named(
                name,
                Some(cadmpeg_ir::design::selection::NativeOwner::Feature(
                    producer.id.clone()
                ))
            )
            .expect("generated edge name")
        );
    }
}
