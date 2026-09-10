// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{CompoundLoftDirection, RevisionCompoundLoftTail};
use crate::ids::CurveId;
use crate::math::Vector3;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TailWire {
    tail: RevisionCompoundLoftTail<CurveId>,
}

#[test]
fn the_revision_compound_loft_tail_names_its_form() {
    let id = CurveId::mint("test:model:curve#tail").expect("valid identity");
    for (tail, expected) in [
        (
            RevisionCompoundLoftTail::Unbounded {},
            json!({"tail": {"kind": "unbounded"}}),
        ),
        (
            RevisionCompoundLoftTail::LowerBound { lower: 2.0 },
            json!({"tail": {"kind": "lower_bound", "lower": 2.0}}),
        ),
        (
            RevisionCompoundLoftTail::UpperBound { upper: 3.0 },
            json!({"tail": {"kind": "upper_bound", "upper": 3.0}}),
        ),
        (
            RevisionCompoundLoftTail::Curve {
                interval: [2.0, 3.0],
                curve: id.clone(),
            },
            json!({"tail": {"kind": "curve", "interval": [2.0, 3.0], "curve": id}}),
        ),
    ] {
        let value = TailWire { tail };
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(serde_json::from_value::<TailWire>(wire).unwrap(), value);
    }
}

#[test]
fn a_detached_bound_and_curve_have_no_encoding_in_the_loft_tail() {
    for tail in [
        json!({"kind": "unbounded", "curve": "test:model:curve#tail"}),
        json!({"kind": "lower_bound", "lower": 2.0, "curve": "test:model:curve#tail"}),
        json!({"kind": "curve", "interval": [2.0, 3.0]}),
        json!({"kind": "curve", "curve": "test:model:curve#tail"}),
    ] {
        assert!(
            serde_json::from_value::<TailWire>(json!({"tail": tail.clone()})).is_err(),
            "{tail}"
        );
    }

    let bogus = json!({"tail": {"kind": "unbounded", "zz_bogus": 1}});
    let error = serde_json::from_value::<TailWire>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn a_compound_loft_direction_carries_a_selector_only_on_its_curve_form() {
    let vector = super::RevisionCompoundLoftDirectionWireTest {
        direction: CompoundLoftDirection::Vector {
            value: Vector3::new(0.0, 0.0, 1.0),
        },
    };
    let wire = serde_json::to_value(&vector).unwrap();
    assert_eq!(wire["direction"]["kind"], "vector");
    assert!(wire["direction"].get("selector").is_none());
    assert_eq!(
        serde_json::from_value::<super::RevisionCompoundLoftDirectionWireTest>(wire).unwrap(),
        vector
    );

    let curve = super::RevisionCompoundLoftDirectionWireTest {
        direction: CompoundLoftDirection::Curve {
            curve: CurveId::mint("test:model:curve#direction").expect("valid identity"),
            selector: std::num::NonZeroI64::new(3).expect("three is nonzero"),
        },
    };
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(wire["direction"]["selector"], 3);
    assert_eq!(
        serde_json::from_value::<super::RevisionCompoundLoftDirectionWireTest>(wire.clone())
            .unwrap(),
        curve
    );

    let mut zero = wire.clone();
    zero["direction"]["selector"] = json!(0);
    assert!(serde_json::from_value::<super::RevisionCompoundLoftDirectionWireTest>(zero).is_err());

    let mut bogus = wire;
    bogus["direction"]["zz_bogus"] = json!(1);
    let error = serde_json::from_value::<super::RevisionCompoundLoftDirectionWireTest>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}
