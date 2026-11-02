// SPDX-License-Identifier: Apache-2.0
//! Distance dimensions measured to an axis line.

use super::{closed_square_inventory, hinted_relation, test_entity, test_line};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::sketch::{
    project, PmDcSketchConstraint, PmDcSketchEntityKind, PmDcSketchEntityPayload, AXIS_LINE_FLAG,
    SKETCH_TYPE,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// The same record read as a distance dimension driven by parameter
/// reference 5.
fn as_distance(relation: &PmDcSketchConstraint) -> PmDcSketchConstraint {
    let mut wire = serde_json::to_value(relation).expect("relation wire");
    wire["kind"] = serde_json::json!({
        "form": "distance",
        "first": wire["kind"]["first"].clone(),
        "second": wire["kind"]["second"].clone(),
        "value": 0,
        "values": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    });
    wire["header"]["parameter"] = serde_json::json!({"index": 5, "qualified": true});
    serde_json::from_value(wire).expect("distance wire")
}

/// Project a distance dimension between entities at ordinals 7 and 3,
/// where the lines named in `axis` carry the axis-line flag.
fn distance_definition(axis: &[u32]) -> serde_json::Value {
    let distance = as_distance(&hinted_relation(0));
    let native = |ordinal: u32| format!("inventor:pmdc:sketch-entity#segment-{ordinal}");
    let first = test_entity(7, test_line()).with_native_ref(Some(native(7)));
    let second = test_entity(3, test_line()).with_native_ref(Some(native(3)));
    let entities =
        std::collections::HashMap::from([(("segment", 7), &first), (("segment", 3), &second)]);
    let parameters = std::collections::HashMap::from([(
        "inventor:pmdc:parameter#segment-4".to_owned(),
        cadmpeg_ir::features::ParameterId::mint("inventor:test:parameter#1").expect("parameter id"),
    )]);
    let axis_lines = axis.iter().map(|ordinal| native(*ordinal)).collect();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projected =
        crate::sketch::project_constraint(&ctx, &distance, &entities, &parameters, &axis_lines)
            .expect("distance operands resolve")
            .expect("distance projection");
    serde_json::to_value(projected.definition).expect("definition wire")
}

#[test]
fn a_projected_distance_to_an_axis_line_is_counted_native() {
    for (flagged, native) in [(true, 1_usize), (false, 0)] {
        let (mut inventory, sketch) = closed_square_inventory();
        if flagged {
            let line = inventory
                .entities
                .iter()
                .position(|entity| matches!(entity.kind, PmDcSketchEntityKind::Line { .. }))
                .expect("square side");
            let mut payload = PmDcSketchEntityPayload::clone(&inventory.entities[line]);
            payload.entity_flags = AXIS_LINE_FLAG;
            assert!(payload.is_axis_line());
            let identity = inventory.entities[line].identity.clone();
            inventory.entities[line] = Located::new(
                payload,
                identity.type_id.clone(),
                identity.segment_token.clone(),
                identity.record_ordinal,
            );
        }
        inventory.constraints.push(as_distance(&hinted_relation(0)));
        let mut sketch = sketch;
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        let parameter = cadmpeg_ir::features::DesignParameter {
            product_definition: None,
            id: cadmpeg_ir::features::ParameterId::mint("inventor:test:parameter#4")
                .expect("parameter id"),
            owner: None,
            ordinal: 0,
            name: "d0".into(),
            expression: "1".into(),
            display: None,
            value: None,
            driving: true,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: Some("inventor:pmdc:parameter#segment-4".into()),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[parameter]).expect("sketch projection");
        assert_eq!(projected.constraints.len(), 1);
        assert_eq!(projected.native_axis_distances, native);
        let definition = serde_json::to_value(&projected.constraints[0].definition).expect("wire");
        assert_eq!(
            definition["kind"],
            if flagged { "native" } else { "distance_loci" }
        );
    }
}

#[test]
fn a_distance_to_an_axis_line_stays_native() {
    assert_eq!(distance_definition(&[])["kind"], "distance_loci");
    for axis in [&[7][..], &[3], &[7, 3]] {
        let definition = distance_definition(axis);
        assert_eq!(definition["kind"], "native", "{definition}");
        assert_eq!(definition["native_kind"], "axis_distance_dimension");
        assert!(definition["parameter"].is_string());
    }
}

#[test]
fn native_axis_distances_are_reported_with_a_pinned_loss() {
    assert!(crate::sketch::axis_distance_loss(0).is_none());
    let note = crate::sketch::axis_distance_loss(3).expect("loss for retained dimensions");
    assert_eq!(note.code.local_code(), "sketch.axis-distance-native");
    assert!(note
        .message
        .contains("3 Inventor sketch distance dimension"));
}
