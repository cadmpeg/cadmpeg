// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{CompoundLoftDirection, RevisionCompoundLoftTail};
use crate::ids::CurveId;
use crate::math::Vector3;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TailWire {
    #[serde(flatten, with = "crate::geometry::revision_compound_loft_tail_wire")]
    tail: RevisionCompoundLoftTail<CurveId>,
}

#[test]
fn revision_compound_loft_tail_preserves_all_four_wire_forms() {
    let id = CurveId::mint("test:model:curve#tail").expect("valid identity");
    for (tail, expected) in [
        (
            RevisionCompoundLoftTail::Unbounded,
            json!({"interval": [null, null]}),
        ),
        (
            RevisionCompoundLoftTail::LowerBound(2.0),
            json!({"interval": [2.0, null]}),
        ),
        (
            RevisionCompoundLoftTail::UpperBound(3.0),
            json!({"interval": [null, 3.0]}),
        ),
        (
            RevisionCompoundLoftTail::Curve {
                interval: [2.0, 3.0],
                curve: id.clone(),
            },
            json!({"interval": [2.0, 3.0], "trailing_curve": id}),
        ),
    ] {
        let value = TailWire { tail };
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(serde_json::from_value::<TailWire>(wire).unwrap(), value);
    }
    assert_eq!(
        serde_json::from_value::<TailWire>(json!({})).unwrap().tail,
        RevisionCompoundLoftTail::Unbounded
    );
}

#[test]
fn revision_compound_loft_tail_rejects_detached_bounds_and_curve() {
    for interval in [json!([null, null]), json!([2.0, null]), json!([null, 3.0])] {
        let wire = json!({"interval": interval, "trailing_curve": "test:model:curve#tail"});
        assert!(serde_json::from_value::<TailWire>(wire).is_err());
    }
    assert!(serde_json::from_value::<TailWire>(json!({"interval": [2.0, 3.0]})).is_err());
}

#[test]
fn revision_compound_loft_rejects_nonzero_kind() {
    let value = super::RevisionCompoundLoftDirectionWireTest {
        direction: CompoundLoftDirection::Vector {
            value: Vector3::new(0.0, 0.0, 1.0),
        },
    };
    let mut wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["kind"], 0);
    wire["kind"] = json!(1);
    let error =
        serde_json::from_value::<super::RevisionCompoundLoftDirectionWireTest>(wire).unwrap_err();
    assert!(error
        .to_string()
        .contains("revision compound loft requires kind zero"));
}
