// SPDX-License-Identifier: Apache-2.0

use super::super::thread_nominal_diameter;
use crate::records::feature::thread::{
    DesignThreadAngle, DesignThreadConstruction, DesignThreadDiameters, DesignThreadForm,
    DesignThreadNominalSize,
};

/// Millimetre tolerance of a converted thread diameter.
const EPS_DIAMETER_MM: f64 = 1e-9;

fn construction(nominal_size: &str, angle: f64) -> DesignThreadConstruction {
    DesignThreadConstruction {
        form: DesignThreadForm::Standard,
        designation_offset: 0,
        designation: cadmpeg_core::text::NonBlankString::new("G1/8").unwrap(),
        nominal_size: DesignThreadNominalSize::try_from(nominal_size.to_owned()).unwrap(),
        profile: cadmpeg_core::text::NonBlankString::new("BSP").unwrap(),
        diameters: DesignThreadDiameters::new(0.9728, 0.8566, 0.9147).unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.0907).unwrap(),
        modeled: false,
        angle: DesignThreadAngle::new(angle).unwrap(),
        face_group_record_indices: vec![20],
        extended: None,
    }
}

fn codes(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

#[test]
fn a_fractional_size_reads_the_major_diameter_and_charges_the_reading() {
    let mut losses = Vec::new();
    let size = thread_nominal_diameter("scope", &construction("1/8", 60.0), &mut losses)
        .expect("thread diameter");
    assert!((size.get() - 9.728).abs() < EPS_DIAMETER_MM);
    assert_eq!(
        codes(&losses),
        [crate::loss::F3dLossCode::ThreadFractionalSizeDiameterAssumed.kind()]
    );
    let mut losses = Vec::new();
    let size = thread_nominal_diameter("scope", &construction("10", 60.0), &mut losses)
        .expect("thread diameter");
    assert!((size.get() - 10.0).abs() < EPS_DIAMETER_MM);
    assert!(losses.is_empty());
}

#[test]
fn a_profile_angle_other_than_sixty_degrees_charges_its_native_retention() {
    let mut losses = Vec::new();
    thread_nominal_diameter("scope", &construction("10", 55.0), &mut losses)
        .expect("thread diameter");
    assert_eq!(
        codes(&losses),
        [crate::loss::F3dLossCode::ThreadAngleNative.kind()]
    );
    assert!(losses[0].message.contains("55-degree"));
}

#[test]
fn numbered_inch_size_projects_its_nominal_diameter_in_millimetres() {
    let mut losses = Vec::new();
    let size = thread_nominal_diameter("scope", &construction("0.216(#12)", 60.0), &mut losses)
        .expect("numbered thread diameter");
    assert!((size.get() - 5.4864).abs() < EPS_DIAMETER_MM);
    assert!(losses.is_empty());
}
