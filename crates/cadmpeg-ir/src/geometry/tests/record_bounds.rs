// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;
use serde::Deserialize;

use crate::geometry::{ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds};
use crate::ids::{ProceduralSurfaceId, SurfaceId};

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("test:geometry:procedural-surface#bounds").expect("valid identity")
}

fn unknown_definition() -> ProceduralSurfaceDefinition {
    ProceduralSurfaceDefinition::Unknown {
        record: None,
        cache: None,
    }
}

#[test]
fn partial_record_bounds_round_trip_through_checked_storage() {
    let bounds = RecordBounds::try_new([Some(0.1), None, Some(0.2), None])
        .expect("finite partial record bounds");
    assert_eq!(bounds.get(), [Some(0.1), None, Some(0.2), None]);

    let wire = serde_json::to_value(bounds).expect("serialize record bounds");
    assert_eq!(wire, serde_json::json!([0.1, null, 0.2, null]));
    assert_eq!(
        serde_json::from_value::<RecordBounds>(wire).expect("deserialize record bounds"),
        bounds
    );
}

#[test]
fn an_all_absent_quartet_is_admitted_and_keeps_its_signed_zeros() {
    let absent = RecordBounds::try_new([None; 4]).expect("an absent quartet states no value");
    assert_eq!(absent.get(), [None; 4]);
    assert_eq!(
        serde_json::to_value(absent).expect("serialize an absent quartet"),
        serde_json::json!([null, null, null, null])
    );

    let signed = RecordBounds::try_new([Some(-0.0), Some(0.0), None, Some(-0.0)])
        .expect("signed zeros are finite");
    let slots = signed.get();
    assert!(slots[0].is_some_and(f64::is_sign_negative));
    assert!(slots[1].is_some_and(f64::is_sign_positive));
    assert!(slots[3].is_some_and(f64::is_sign_negative));
    assert_eq!(
        serde_json::from_value::<RecordBounds>(
            serde_json::to_value(signed).expect("serialize signed zeros")
        )
        .expect("deserialize signed zeros")
        .get(),
        slots
    );
}

#[test]
fn every_slot_refuses_a_nonfinite_value_through_construction_and_deserialization() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for slot in 0..4 {
            let mut raw = [0.5_f64; 4];
            raw[slot] = value;
            assert!(RecordBounds::try_new(raw.map(Some)).is_err());
            let deserializer = serde::de::value::SeqDeserializer::<_, serde::de::value::Error>::new(
                raw.into_iter(),
            );
            assert!(RecordBounds::deserialize(deserializer).is_err());
        }
    }
}

#[test]
fn the_containing_row_separates_an_absent_quartet_from_a_stated_one() {
    let surface = SurfaceId::mint("test:model:surface#bounds").expect("valid identity");
    let without = crate::geometry::ProceduralSurfaceRow::new(
        surface.clone(),
        &ProceduralSurface::new(id(), unknown_definition(), None),
    );
    let wire = serde_json::to_value(&without).expect("serialize a row without bounds");
    assert!(wire.get("record_bounds").is_none());

    let with = crate::geometry::ProceduralSurfaceRow::new(
        surface,
        &ProceduralSurface::new(
            id(),
            unknown_definition(),
            Some(RecordBounds::try_new([None; 4]).expect("an absent quartet")),
        ),
    );
    let wire = serde_json::to_value(&with).expect("serialize a row with an absent quartet");
    assert_eq!(
        wire["record_bounds"],
        serde_json::json!([null, null, null, null])
    );

    let mut null_key = wire.clone();
    null_key["record_bounds"] = serde_json::Value::Null;
    let Err(refusal) = serde_json::from_value::<crate::geometry::ProceduralSurfaceRow>(null_key)
    else {
        panic!("an explicit null is not the absent spelling");
    };
    assert!(refusal.to_string().contains("record_bounds"), "{refusal}");
}

#[test]
fn non_finite_record_bounds_are_rejected_without_mutating_the_surface() {
    let mut surface = ProceduralSurface::new(
        id(),
        unknown_definition(),
        Some(
            RecordBounds::try_new([Some(0.1), Some(0.9), None, None])
                .expect("finite initial bounds"),
        ),
    );

    assert!({
        let replacement = Some([Some(f64::NAN), None, Some(0.2), None]);
        edit::replace(&mut surface, |previous| {
            replacement
                .map(RecordBounds::try_from)
                .transpose()
                .map(|bounds| {
                    ProceduralSurface::new(
                        previous.id.clone(),
                        previous.definition().clone(),
                        bounds,
                    )
                })
        })
    }
    .is_err());
    assert_eq!(
        surface.record_bounds().map(RecordBounds::get),
        Some([Some(0.1), Some(0.9), None, None])
    );
    assert!(RecordBounds::try_new([Some(f64::INFINITY), None, None, None]).is_err());
}
