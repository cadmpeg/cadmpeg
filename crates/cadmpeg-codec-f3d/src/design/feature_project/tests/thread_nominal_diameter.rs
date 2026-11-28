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
        designation: cadmpeg_core::text::NonBlankString::new(if nominal_size == "10" {
            "M10"
        } else {
            "G1/8"
        })
        .unwrap(),
        nominal_size: DesignThreadNominalSize::try_from(nominal_size.to_owned()).unwrap(),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
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

#[test]
fn half_inch_fraction_uses_the_physical_major_diameter() {
    let mut construction = construction("1/2", 60.0);
    construction.designation = cadmpeg_core::text::NonBlankString::new("1/2-28 UNEF").unwrap();
    construction.profile =
        cadmpeg_core::text::NonBlankString::new("ANSI Unified Screw Threads").unwrap();
    construction.diameters = DesignThreadDiameters::new(1.27, 1.1, 1.2).unwrap();
    let mut losses = Vec::new();
    let size = thread_nominal_diameter("scope", &construction, &mut losses).unwrap();
    assert!((size.get() - 12.7).abs() < EPS_DIAMETER_MM);
    assert_eq!(
        codes(&losses),
        [crate::loss::F3dLossCode::ThreadFractionalSizeDiameterAssumed.kind()]
    );
}

#[test]
fn decimal_size_with_unproven_profile_unit_stays_unresolved() {
    for profile in ["ANSI Unified Screw Threads", "unassigned profile"] {
        let mut construction = construction("0.5", 60.0);
        construction.profile = cadmpeg_core::text::NonBlankString::new(profile).unwrap();
        let mut losses = Vec::new();
        assert!(thread_nominal_diameter("scope", &construction, &mut losses).is_none());
        assert_eq!(
            codes(&losses),
            [crate::loss::F3dLossCode::ThreadNominalUnitUnresolved.kind()]
        );
    }
}

#[test]
fn metric_nominal_size_stays_in_millimetres() {
    let mut construction = construction("6", 60.0);
    construction.designation = cadmpeg_core::text::NonBlankString::new("M6x1").unwrap();
    let mut losses = Vec::new();
    assert_eq!(
        thread_nominal_diameter("scope", &construction, &mut losses)
            .unwrap()
            .get(),
        6.0
    );
    assert!(losses.is_empty());
}

#[test]
fn decimal_nominal_size_requires_designation_agreement() {
    for (designation, nominal, profile, expected) in [
        (
            "1/2-28 UNEF",
            "0.5",
            "ANSI Unified Screw Threads",
            Some(12.7),
        ),
        (
            "1/4-20 UNC",
            "0.25",
            "ANSI Unified Screw Threads",
            Some(6.35),
        ),
        (
            "#10-32 UNF",
            "0.19",
            "ANSI Unified Screw Threads",
            Some(4.826),
        ),
        ("0.5-28 UNEF", "0.5", "unassigned profile", Some(12.7)),
        ("1-8 UNC", "1", "unassigned profile", Some(25.4)),
        ("M6x1", "6", "ISO Metric profile", Some(6.0)),
        ("1/2-28 UNEF", "0.25", "ANSI Unified Screw Threads", None),
        ("unspecified", "0.5", "unassigned profile", None),
        ("1/2-28 UNEF", "0.5", "ISO Metric profile", None),
        ("M6x1", "6", "ANSI Unified Screw Threads", None),
        ("0.5-28 OTHER", "0.5", "unassigned profile", None),
    ] {
        let construction: DesignThreadConstruction =
            cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
                "form": "standard", "designation_offset": 0,
                "designation": designation, "nominal_size_text": nominal,
                "nominal_size": nominal.parse::<f64>().unwrap(), "profile": profile,
                "major_diameter": 1.27, "minor_diameter": 1.1,
                "pitch_diameter": 1.2, "pitch": 2.54 / 28.0,
                "modeled": false, "face_group_record_indices": [10]
            }))
            .unwrap();
        let mut losses = Vec::new();
        let actual = thread_nominal_diameter("scope", &construction, &mut losses);
        match expected {
            Some(expected) => {
                assert!((actual.unwrap().get() - expected).abs() < EPS_DIAMETER_MM);
                assert!(losses.is_empty());
            }
            None => {
                assert!(actual.is_none());
                assert_eq!(
                    codes(&losses),
                    [crate::loss::F3dLossCode::ThreadNominalUnitUnresolved.kind()]
                );
            }
        }
    }
}

#[test]
fn absent_designation_has_no_decimal_unit() {
    assert!(
        super::super::thread_units::decimal_millimetres("", "ANSI Unified Screw Threads", 0.5)
            .is_none()
    );
}
