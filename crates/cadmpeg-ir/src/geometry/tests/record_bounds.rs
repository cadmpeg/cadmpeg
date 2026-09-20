// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;
use serde::{
    de::{value::Error, value::F64Deserializer, value::SeqDeserializer, IntoDeserializer, Visitor},
    Deserialize, Deserializer,
};

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

// Supply actual optional numbers, including values JSON cannot represent.
struct BoundSlot(Option<f64>);

impl<'de> IntoDeserializer<'de, Error> for BoundSlot {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> Deserializer<'de> for BoundSlot {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Some(value) => visitor.visit_some(F64Deserializer::<Error>::new(value)),
            None => visitor.visit_none(),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

fn deserialize_bounds(raw: [Option<f64>; 4]) -> Result<RecordBounds, Error> {
    RecordBounds::deserialize(SeqDeserializer::new(raw.into_iter().map(BoundSlot)))
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
    assert_eq!(deserialize_bounds([None; 4]).unwrap(), absent);
    assert_eq!(
        serde_json::from_str::<RecordBounds>("[null,null,null,null]").unwrap(),
        absent
    );
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
        .get()
        .map(|slot| slot.map(f64::to_bits)),
        slots.map(|slot| slot.map(f64::to_bits))
    );
}

#[test]
fn every_slot_refuses_a_nonfinite_value_through_construction_and_deserialization() {
    for raw in [[Some(0.5); 4], [Some(-0.0), None, Some(0.5), Some(0.0)]] {
        assert_eq!(
            deserialize_bounds(raw)
                .unwrap()
                .get()
                .map(|slot| slot.map(f64::to_bits)),
            raw.map(|slot| slot.map(f64::to_bits))
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for slot in 0..4 {
            let mut raw = [Some(0.5); 4];
            raw[slot] = Some(value);
            assert!(RecordBounds::try_new(raw).is_err());
            assert!(RecordBounds::try_from(raw).is_err());
            assert_eq!(
                deserialize_bounds(raw).unwrap_err().to_string(),
                "record bounds must contain only finite values"
            );
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
    let decoded: crate::geometry::ProceduralSurfaceRow = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.into_parts().1.record_bounds(), None);

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
    let decoded: crate::geometry::ProceduralSurfaceRow =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        decoded.into_parts().1.record_bounds().unwrap().get(),
        [None; 4]
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
fn the_direct_surface_requires_the_bounds_key_and_accepts_null() {
    for raw in [
        None,
        Some([None; 4]),
        Some([Some(-0.0), None, Some(2.0), None]),
    ] {
        let surface = ProceduralSurface::new(
            id(),
            unknown_definition(),
            raw.map(RecordBounds::try_from).transpose().unwrap(),
        );
        let mut wire = serde_json::to_value(&surface).unwrap();
        assert!(wire.get("record_bounds").is_some());
        if raw.is_none() {
            assert!(wire["record_bounds"].is_null());
        }
        let decoded: ProceduralSurface = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            decoded
                .record_bounds()
                .map(|bounds| bounds.get().map(|slot| slot.map(f64::to_bits))),
            raw.map(|bounds| bounds.map(|slot| slot.map(f64::to_bits)))
        );
        wire.as_object_mut().unwrap().remove("record_bounds");
        let error = serde_json::from_value::<ProceduralSurface>(wire).unwrap_err();
        assert!(
            error.to_string().contains("missing field `record_bounds`"),
            "{error}"
        );
    }
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
