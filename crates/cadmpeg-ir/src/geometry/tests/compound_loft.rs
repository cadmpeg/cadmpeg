// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{CompoundLoftDirection, CompoundLoftTail};

#[test]
fn direction_selector_round_trips_without_duplicate_state() {
    for selector in [0, 1, 4, -3] {
        let direction = if selector == 0 {
            CompoundLoftDirection::Vector {
                value: crate::math::Vector3::new(1.0, 0.0, 0.0),
            }
        } else {
            CompoundLoftDirection::Curve {
                curve: "test:model:curve#0".into(),
                selector: std::num::NonZeroI64::new(selector).unwrap(),
            }
        };
        let tail = CompoundLoftTail::Zero {
            flags: [true, false],
            direction,
            trailing_flags: [false, true],
        };
        let mut wire = serde_json::to_value(&tail).unwrap();
        assert_eq!(wire["selector"], selector);
        assert_eq!(
            serde_json::from_value::<CompoundLoftTail>(wire.clone()).unwrap(),
            tail
        );
        wire["selector"] = serde_json::json!(if selector == 0 { 4 } else { 0 });
        assert!(serde_json::from_value::<CompoundLoftTail>(wire).is_err());
    }
}

#[test]
fn scaled_direction_selector_preserves_exact_nonzero_value() {
    use crate::geometry::ScaledCompoundLoftBranch;
    let branch = ScaledCompoundLoftBranch::Direct {
        flag: true,
        direction: CompoundLoftDirection::Curve {
            curve: "test:model:curve#0".into(),
            selector: std::num::NonZeroI64::new(-4).unwrap(),
        },
    };
    let mut wire = serde_json::to_value(&branch).unwrap();
    assert_eq!(wire["selector"], -4);
    assert_eq!(
        serde_json::from_value::<ScaledCompoundLoftBranch>(wire.clone()).unwrap(),
        branch
    );
    wire["selector"] = serde_json::json!(0);
    assert!(serde_json::from_value::<ScaledCompoundLoftBranch>(wire).is_err());
}
