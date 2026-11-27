// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::recognition_floor_loss;
use cadmpeg_ir::scalar::{PositiveAngle, PositiveLength};
use cadmpeg_ir::units::Tolerances;

fn tolerances(linear_mm: f64) -> Tolerances {
    Tolerances {
        linear: PositiveLength::new(linear_mm).expect("positive length"),
        angular: PositiveAngle::new(1.0e-11).expect("positive angle"),
    }
}

#[test]
fn a_tolerance_below_the_floor_is_charged_where_a_recogniser_runs() {
    let note = recognition_floor_loss(&tolerances(1.0e-10), true).expect("charged");
    assert_eq!(
        note.code,
        crate::loss::NxLossCode::FeatureRecognitionToleranceFloor.kind()
    );
}

#[test]
fn a_tolerance_below_the_floor_is_not_charged_where_no_recogniser_runs() {
    assert!(recognition_floor_loss(&tolerances(1.0e-10), false).is_none());
}

#[test]
fn a_tolerance_at_or_above_the_floor_is_not_charged() {
    assert!(recognition_floor_loss(&tolerances(1.0e-9), true).is_none());
    assert!(recognition_floor_loss(&tolerances(1.0e-5), true).is_none());
}
