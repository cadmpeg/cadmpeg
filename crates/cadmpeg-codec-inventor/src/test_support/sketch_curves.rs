// SPDX-License-Identifier: Apache-2.0
//! Synthetic planar curves and midpoint controls.

use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};
use crate::sketch::{PmDcSketchEntity, PmDcSketchEntityPayload};
use crate::test_support::test_fixtures::{content_at, parse};

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn list(bytes: &mut Vec<u8>, values: &[u32]) {
    word(bytes, 0x3000_0002);
    word(bytes, u32::try_from(values.len()).expect("synthetic count"));
    if !values.is_empty() {
        word(bytes, 2);
        word(bytes, 0);
        for value in values {
            word(bytes, *value);
        }
    }
}

fn cubic(bytes: &mut Vec<u8>, pole_count: u32, t0: f64) {
    word(bytes, 0);
    word(bytes, 3);
    real(bytes, 1e-9);
    for value in [pole_count + 4, pole_count + 4, 8] {
        word(bytes, value);
    }
    for _ in 0..4 {
        real(bytes, t0);
    }
    for index in 1..=pole_count - 4 {
        real(bytes, t0 + f64::from(index) / f64::from(pole_count - 3));
    }
    for _ in 0..4 {
        real(bytes, t0 + 1.0);
    }
    for value in [0, 0, 8, pole_count, 8, 8] {
        word(bytes, value);
    }
    for index in 0..pole_count {
        let point = if index == 0 {
            [0.0, 0.0]
        } else if index == pole_count - 1 {
            [2.0, 0.0]
        } else {
            [f64::from(index - 1), 2.0]
        };
        for value in point {
            real(bytes, value);
        }
    }
    real(bytes, 1e-12);
    for value in [1, 1] {
        word(bytes, value);
    }
    for value in [t0, t0 + 1.0, 0.0] {
        real(bytes, value);
    }
    word(bytes, 8);
}

pub(crate) fn open_cubic_bytes() -> Vec<u8> {
    cubic_bytes(4, 0.0)
}

fn cubic_bytes(pole_count: u32, t0: f64) -> Vec<u8> {
    assert!(pole_count >= 4);
    let mut bytes = content_at(8, 23, &[-1]);
    word(&mut bytes, 0x40);
    word(&mut bytes, 3);
    list(&mut bytes, &[4, 5]);
    for value in [0x3000_0002, 0, 0, 0, 0, 0, 0, 0, 0] {
        word(&mut bytes, value);
    }
    list(&mut bytes, &[0, 1]);
    word(&mut bytes, 0);
    cubic(&mut bytes, pole_count, t0);
    bytes.push(2);
    list(&mut bytes, &[]);
    list(&mut bytes, &[]);
    for _ in 0..3 {
        word(&mut bytes, 0);
    }
    bytes.push(0);
    word(&mut bytes, 1);
    bytes.push(0);
    cubic(&mut bytes, pole_count, t0);
    for value in [2, 0, 0, 0, 0, 0, 0, 0, 0] {
        word(&mut bytes, value);
    }
    bytes
}

fn located(payload: PmDcSketchEntityPayload, ordinal: u32) -> PmDcSketchEntity {
    let kind = RecordTypeId::from_bytes(payload.kind.record_type_id());
    Located::new(
        payload,
        kind,
        cadmpeg_ir::identity_key!("generated"),
        ordinal,
    )
}

pub(crate) fn open_cubic_entities() -> Vec<PmDcSketchEntity> {
    open_cubic_entities_with(4, 0.0)
}

pub(crate) fn open_cubic_entities_with(pole_count: u32, t0: f64) -> Vec<PmDcSketchEntity> {
    let spline = parse(&cubic_bytes(pole_count, t0), |ctx, source| {
        crate::sketch::spline::parse(ctx, source, 23).expect("synthetic open cubic")
    });
    let point = |ordinal, position: [f64; 2]| {
        let list = serde_json::json!({"marker": 2, "metadata": null, "references": []});
        let payload = serde_json::from_value(serde_json::json!({
            "save_version_major": 23,
            "header": {"header_value": 0, "header_id": 0,
                "next": PmDcReference::from_packed(0), "flags": 0,
                "context": PmDcReference::from_packed(3), "source_index": 0,
                "stamps": [-1]},
            "entity_flags": 0x40, "sketch": PmDcReference::from_packed(3),
            "kind": {"form": "point", "position": position,
                "endpoint_of": list, "center_of": list, "state": null, "associations": null}
        }))
        .expect("synthetic spline endpoint");
        located(payload, ordinal)
    };
    vec![
        point(3, [0.0, 0.0]),
        point(4, [2.0, 0.0]),
        located(spline, 7),
    ]
}

pub(crate) fn spline_midpoint() -> crate::sketch::PmDcSketchConstraint {
    let payload = serde_json::from_value(serde_json::json!({
        "save_version_major": 23,
        "header": {
            "content": {"header_value": 0, "header_id": 0,
                "next": PmDcReference::from_packed(0), "flags": 0,
                "context": PmDcReference::from_packed(3), "source_index": 0},
            "state": -1, "group": PmDcReference::from_packed(3),
            "scalar_map": {"metadata": null, "entries": []},
            "reference_map": {"metadata": null, "entries": []},
            "parameter": PmDcReference::from_packed(0)
        },
        "kind": {"form": "midpoint", "entity": PmDcReference::from_packed(8),
            "point": PmDcReference::from_packed(4), "value": 0}
    }))
    .expect("synthetic spline midpoint");
    Located::new(
        payload,
        RecordTypeId::try_from("bf70e821d011d0d20008ccbc0663dc09".to_owned())
            .expect("midpoint type"),
        cadmpeg_ir::identity_key!("generated"),
        9,
    )
}

pub(crate) fn edit_entity(
    entity: &mut PmDcSketchEntity,
    edit: impl FnOnce(&mut PmDcSketchEntityPayload),
) {
    let mut payload = PmDcSketchEntityPayload::clone(entity);
    edit(&mut payload);
    *entity = Located::new(
        payload,
        entity.identity.type_id.clone(),
        entity.identity.segment_token.clone(),
        entity.identity.record_ordinal,
    );
}

pub(crate) fn line_midpoint_entities() -> Vec<PmDcSketchEntity> {
    let mut entities = open_cubic_entities();
    let mut start = entities[0].clone();
    start.identity.record_ordinal = 5;
    edit_entity(&mut start, |payload| {
        let crate::sketch::PmDcSketchEntityKind::Point { position, .. } = &mut payload.kind else {
            panic!("synthetic point");
        };
        position[0] = cadmpeg_ir::scalar::FiniteReal::new(-2.0).expect("start coordinate");
    });
    edit_entity(&mut entities[2], |payload| {
        payload.kind = crate::sketch::PmDcSketchEntityKind::Line {
            points: crate::pmdc::PmDcReferenceList::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([2, 0])),
                vec![PmDcReference::from_packed(6), PmDcReference::from_packed(5)],
            )
            .expect("line endpoints"),
            auxiliary: Vec::new(),
            origin: [cadmpeg_ir::scalar::FiniteReal::ZERO; 2],
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
        };
    });
    entities[2].identity.type_id =
        RecordTypeId::try_from("3adf52ced011d0d20008ccbc0663dc09".to_owned())
            .expect("line record type");
    entities.push(start);
    entities
}
