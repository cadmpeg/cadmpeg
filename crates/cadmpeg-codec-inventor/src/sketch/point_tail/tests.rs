// SPDX-License-Identifier: Apache-2.0

use crate::pmdc::PmDcReference;
use crate::sketch::{parse_entity, PmDcSketchEntityKind, SketchEntityTag};
use crate::sketch::{PointTail, PointTailWire};
use crate::test_support::test_fixtures::{content_at, parse};
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;

#[derive(serde::Serialize)]
struct Record<'a> {
    id: &'static str,
    value: &'a PointTail,
}

fn pair_header(bytes: &mut Vec<u8>, metadata: [u32; 2]) {
    for value in [0x3000_0002, 2, metadata[0], metadata[1]] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn paired_point(version: u8) -> (Vec<u8>, [usize; 4]) {
    let stamps = if version >= 25 {
        &[-1, 2500][..]
    } else {
        &[][..]
    };
    let mut bytes = content_at(3, version, stamps);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for coordinate in [0.0f64, 0.0] {
        bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    let tail = bytes.len();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let scalar_header = bytes.len();
    pair_header(&mut bytes, [2, 0]);
    for scalar in [0.25f64, 0.75] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }
    let operand_header = bytes.len();
    pair_header(&mut bytes, [2, 7]);
    let auxiliary = bytes.len();
    for reference in [0x8000_0008u32, 0x8000_000b] {
        bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&reference.to_le_bytes());
    }
    (bytes, [tail, scalar_header, operand_header, auxiliary])
}

#[test]
fn paired_point_tail_has_two_scalars_and_two_operands() {
    for version in [25, 28, 29, 30, 31] {
        let (bytes, [tail_start, ..]) = paired_point(version);
        assert_eq!(bytes.len() - tail_start, 76);
        let point = parse(&bytes, |_, source| {
            parse_entity(SketchEntityTag::Point, source, version).expect("paired point")
        });
        let point_wire = serde_json::to_value(&point.kind).expect("paired point wire");
        assert!(point_wire.get("links").is_none());
        let mut invalid = point_wire.clone();
        invalid["links"] = serde_json::json!({
            "marker": 2, "metadata": null, "references": []
        });
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(invalid).is_err());
        assert_eq!(
            serde_json::from_value::<PmDcSketchEntityKind>(point_wire)
                .expect("paired point round trip"),
            point.kind
        );
        let PmDcSketchEntityKind::Point {
            position,
            tail: PointTail::Paired(paired),
            ..
        } = point.kind
        else {
            panic!("paired association tail")
        };
        assert_eq!(position.map(FiniteReal::get), [0.0, 0.0]);
        assert_eq!(paired.scalars.map(FiniteReal::get), [0.25, 0.75]);
        assert_eq!(paired.scalar_metadata, [2, 0]);
        assert_eq!(paired.operand_metadata, [2, 7]);
        assert_eq!(
            paired.operands,
            [
                PmDcReference::from_packed(0x8000_0008),
                PmDcReference::from_packed(0x8000_000b),
            ]
        );
        let tail = PointTail::Paired(paired);
        let wire = serde_json::to_value(&tail).expect("tail wire");
        assert_eq!(wire["state"], 2);
        assert!(wire["associations"].is_null());
        assert_eq!(
            serde_json::from_value::<PointTail>(wire.clone()).expect("tail round trip"),
            tail
        );
        let owned = PointTailWire::from(tail.clone());
        assert_eq!(serde_json::to_value(owned).expect("owned tail wire"), wire);
        let record = Record {
            id: "inventor:pmdc:point-tail#1",
            value: &tail,
        };
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": wire}),
        );
        let mut invalid = wire.clone();
        invalid["state"] = serde_json::json!(0);
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["paired"]["scalars"] = serde_json::json!([0.25]);
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["paired"]["operands"] = serde_json::json!([]);
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        for field in ["scalar_metadata", "operand_metadata"] {
            let mut invalid = wire.clone();
            invalid["paired"][field] = serde_json::json!([0]);
            assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        }
        let mut invalid = wire.clone();
        invalid["paired"]["scalars"][0] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["paired"]["operands"][0]["index"] = serde_json::json!(0x8000_0000u32);
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["paired"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
        let mut invalid = wire;
        invalid["associations"] = serde_json::json!({
            "marker": 2, "metadata": null, "references": []
        });
        assert!(serde_json::from_value::<PointTail>(invalid).is_err());
    }
}

#[test]
fn ordinary_and_absent_point_wires_keep_their_shape() {
    let list = serde_json::json!({"marker": 2, "metadata": null, "references": []});
    for (state, associations, links) in [
        (serde_json::Value::Null, serde_json::Value::Null, None),
        (serde_json::json!(0), list.clone(), None),
        (serde_json::json!(2), list.clone(), None),
        (serde_json::json!(0), list.clone(), Some(list.clone())),
    ] {
        let mut wire = serde_json::json!({
            "form": "point", "position": [0.0, 0.0],
            "endpoint_of": list, "center_of": list,
            "state": state, "associations": associations
        });
        if let Some(links) = links {
            wire["links"] = links;
        }
        let point = serde_json::from_value::<PmDcSketchEntityKind>(wire.clone())
            .expect("ordinary or absent point");
        assert_eq!(serde_json::to_value(point).expect("point wire"), wire);
        wire["paired"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire).is_err());
    }
    let mut absent = serde_json::json!({
        "form": "point", "position": [0.0, 0.0],
        "endpoint_of": list, "center_of": list,
        "state": null, "associations": null, "links": list
    });
    assert!(serde_json::from_value::<PmDcSketchEntityKind>(absent.clone()).is_err());
    absent["links"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<PmDcSketchEntityKind>(absent).is_err());
}

#[test]
fn legacy_state_two_keeps_the_ordinary_association_list() {
    for version in [15, 22, 23] {
        let stamps = if version == 23 { &[-1][..] } else { &[][..] };
        let mut bytes = content_at(3, version, stamps);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        for coordinate in [0.0f64, 0.0] {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
        for _ in 0..2 {
            bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for value in [2u32, 0x3000_0002, 1, 0, 0, 0x8000_0008] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let point = parse(&bytes, |_, source| {
            parse_entity(SketchEntityTag::Point, source, version).expect("legacy ordinary point")
        });
        let wire = serde_json::to_value(&point.kind).expect("legacy point wire");
        assert!(wire.get("paired").is_none());
        assert!(wire.get("links").is_none());
        assert_eq!(
            serde_json::from_value::<PmDcSketchEntityKind>(wire).expect("legacy point round trip"),
            point.kind
        );
        assert!(matches!(
            point.kind,
            PmDcSketchEntityKind::Point {
                tail: PointTail::Present { state: 2, associations, links: None }, ..
            } if associations.references() == [PmDcReference::from_packed(0x8000_0008)]
        ));
    }
}

#[test]
fn paired_point_tail_requires_complete_finite_exact_lists() {
    let (bytes, [tail, scalar_header, operand_header, auxiliary]) = paired_point(25);
    for length in tail + 1..bytes.len() {
        assert!(parse_entity(
            SketchEntityTag::Point,
            View::over_retained(&bytes[..length]),
            25
        )
        .is_err());
    }
    for offset in [scalar_header, operand_header] {
        for (relative, value) in [
            (0, 0x3000_0006u32),
            (0, 2),
            (4, 0),
            (4, 1),
            (4, 3),
            (4, u32::MAX),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset + relative..offset + relative + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                parse_entity(SketchEntityTag::Point, View::over_retained(&invalid), 25).is_err()
            );
        }
    }
    for offset in [scalar_header + 16, scalar_header + 24] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            assert!(
                parse_entity(SketchEntityTag::Point, View::over_retained(&invalid), 25).is_err()
            );
        }
    }
    for offset in [auxiliary, auxiliary + 12] {
        for (relative, value) in [(0, 0x3000_0006u32), (0, 2), (4, 1), (4, u32::MAX)] {
            let mut invalid = bytes.clone();
            invalid[offset + relative..offset + relative + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                parse_entity(SketchEntityTag::Point, View::over_retained(&invalid), 25).is_err()
            );
        }
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(parse_entity(SketchEntityTag::Point, View::over_retained(&trailing), 25).is_err());
    let (older, _) = paired_point(22);
    assert!(parse_entity(SketchEntityTag::Point, View::over_retained(&older), 22).is_err());
}

#[test]
fn paired_point_scalars_are_not_limited_to_unit_parameters() {
    let (mut bytes, [_, scalar_header, ..]) = paired_point(25);
    for (offset, value) in [(scalar_header + 16, -2.0f64), (scalar_header + 24, 5.0)] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let point = parse(&bytes, |_, source| {
        parse_entity(SketchEntityTag::Point, source, 25).expect("paired point")
    });
    let PmDcSketchEntityKind::Point {
        tail: PointTail::Paired(paired),
        ..
    } = point.kind
    else {
        panic!("paired association tail")
    };
    assert_eq!(paired.scalars.map(FiniteReal::get), [-2.0, 5.0]);
}
