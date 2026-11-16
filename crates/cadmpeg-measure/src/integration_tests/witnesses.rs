// SPDX-License-Identifier: Apache-2.0
//! Witnesses of declared-orientation faces: tangent edges in a capsule and
//! the shell sign of a hemisphere solid, with expectations from geometry.

use std::f64::consts::{FRAC_PI_2, PI};

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::{CaveatCode, UnavailableReason};
use crate::report::{BodyMeasures, FaceMeasures, MeasureReport, WitnessKind};
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, measure, Builder, Circle, Frame, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Quantity, Reading};

const RADIUS: f64 = 5.0;
const LENGTH: f64 = 10.0;

/// Two quarter-sphere lunes on separate surface records closed by a disk in
/// the plane `y = 0`. `flip` swaps the loop direction of both lunes, which
/// makes each lune's declared reading the rest of the sphere.
fn split_half_ball(flip: bool) -> CadIr {
    let mut builder = Builder::new();
    let south = builder.vertex([0.0, 0.0, -RADIUS]);
    let north = builder.vertex([0.0, 0.0, RADIUS]);
    let meridians: Vec<_> = [PI, 1.5 * PI, 0.0]
        .into_iter()
        .map(|longitude| {
            let (sine, cosine) = longitude.sin_cos();
            builder.arc(
                Circle {
                    frame: Frame {
                        origin: [0.0; 3],
                        axis: [sine, -cosine, 0.0],
                        reference: [cosine, sine, 0.0],
                    },
                    radius: RADIUS,
                },
                [-FRAC_PI_2, FRAC_PI_2],
                &south,
                &north,
            )
        })
        .collect();
    let (ahead, behind) = if flip {
        (Sense::Reversed, Sense::Forward)
    } else {
        (Sense::Forward, Sense::Reversed)
    };
    for pair in meridians.windows(2) {
        let sphere = builder.sphere(Frame::z_up([0.0; 3]), RADIUS);
        builder.face(
            &sphere,
            Sense::Forward,
            &[vec![(pair[1].clone(), ahead), (pair[0].clone(), behind)]],
            false,
        );
    }
    let disk = builder.plane([0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    builder.face(
        &disk,
        Sense::Forward,
        &[vec![
            (meridians[0].clone(), Sense::Forward),
            (meridians[2].clone(), Sense::Reversed),
        ]],
        false,
    );
    builder.finish(BodyKind::Solid)
}

#[test]
fn linked_lunes_of_one_sphere_confirm_their_declared_readings() {
    let ir = split_half_ball(false);
    let report = measure(&ir, &Selection::default());
    let body = only_body(&report);
    let volume = 2.0 * PI * RADIUS.powi(3) / 3.0;
    assert_measured(body.volume().as_ref().expect("volume"), volume, volume);
    assert_measured(
        body.area().as_ref().expect("area"),
        3.0 * PI * RADIUS.powi(2),
        100.0,
    );
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    assert_eq!(domes.len(), 2);
    for dome in domes {
        assert_eq!(*dome.orientation().consistent(), Some(true));
        let witness = dome.orientation().witness().as_ref().expect("witness");
        assert_eq!(*witness.kind(), WitnessKind::SharedSphere);
        assert!(*witness.agrees());
        assert_measured(dome.measures().area(), PI * RADIUS * RADIUS, 100.0);
        let area = dome.measures().area().measured().expect("area");
        assert_eq!(area.reading(), None);
        assert!(area.candidates().is_empty());
    }
}

#[test]
fn linked_lunes_with_too_much_area_contradict_their_declared_readings() {
    let ir = split_half_ball(true);
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    assert_eq!(domes.len(), 2);
    for dome in domes {
        assert_eq!(*dome.orientation().consistent(), Some(false));
        let witness = dome.orientation().witness().as_ref().expect("witness");
        assert_eq!(*witness.kind(), WitnessKind::SharedSphere);
        assert!(!*witness.agrees());
    }
}

#[test]
fn linked_lunes_covering_the_sphere_stay_unconfirmed() {
    let mut builder = Builder::new();
    let south = builder.vertex([0.0, 0.0, -RADIUS]);
    let north = builder.vertex([0.0, 0.0, RADIUS]);
    let meridians: Vec<_> = [0.0, PI]
        .into_iter()
        .map(|longitude| {
            let (sine, cosine) = longitude.sin_cos();
            builder.arc(
                Circle {
                    frame: Frame {
                        origin: [0.0; 3],
                        axis: [sine, -cosine, 0.0],
                        reference: [cosine, sine, 0.0],
                    },
                    radius: RADIUS,
                },
                [-FRAC_PI_2, FRAC_PI_2],
                &south,
                &north,
            )
        })
        .collect();
    for (ahead, behind) in [(1, 0), (0, 1)] {
        let sphere = builder.sphere(Frame::z_up([0.0; 3]), RADIUS);
        builder.face(
            &sphere,
            Sense::Forward,
            &[vec![
                (meridians[ahead].clone(), Sense::Forward),
                (meridians[behind].clone(), Sense::Reversed),
            ]],
            false,
        );
    }
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    assert_eq!(domes.len(), 2);
    for dome in domes {
        assert!(dome.orientation().witness().is_none());
    }
}

fn only_body(report: &MeasureReport) -> &BodyMeasures {
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body
}

fn sphere_faces(report: &MeasureReport) -> Vec<&FaceMeasures> {
    report
        .faces()
        .iter()
        .filter(|face| face.carrier().kind().as_str() == "sphere")
        .collect()
}

fn everything() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

/// A cylinder of radius 5 from `z = 0` to `z = 10` closed by two
/// hemispheres; the top dome has `top` as its sense.
fn capsule(top: Sense) -> CadIr {
    let mut builder = Builder::new();
    let low = builder.vertex([RADIUS, 0.0, 0.0]);
    let high = builder.vertex([RADIUS, 0.0, LENGTH]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &low);
    let rim = builder.circle(
        [0.0, 0.0, LENGTH],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        RADIUS,
        &high,
    );
    let cylinder = builder.cylinder(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &cylinder,
        Sense::Forward,
        &[
            vec![(bottom.clone(), Sense::Forward)],
            vec![(rim.clone(), Sense::Reversed)],
        ],
        false,
    );
    let upper = builder.sphere(Frame::z_up([0.0, 0.0, LENGTH]), RADIUS);
    builder.face(&upper, top, &[vec![(rim, Sense::Forward)]], true);
    let lower = builder.sphere(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &lower,
        Sense::Forward,
        &[vec![(bottom, Sense::Reversed)]],
        true,
    );
    builder.finish(BodyKind::Solid)
}

#[test]
fn tangent_edges_confirm_the_domes_of_a_capsule() {
    let ir = capsule(Sense::Forward);
    let report = measure(&ir, &Selection::default());
    let body = only_body(&report);
    let volume = PI * RADIUS * RADIUS * LENGTH + 4.0 * PI * RADIUS.powi(3) / 3.0;
    assert_measured(body.volume().as_ref().expect("volume"), volume, volume);
    assert_measured(
        body.area().as_ref().expect("area"),
        2.0 * PI * RADIUS * LENGTH + 4.0 * PI * RADIUS * RADIUS,
        100.0,
    );
    assert_eq!(*body.orientation().consistent(), Some(true));
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    assert_eq!(domes.len(), 2);
    for dome in domes {
        let witness = dome.orientation().witness().as_ref().expect("witness");
        assert_eq!(*witness.kind(), WitnessKind::TangentEdge);
        assert!(*witness.agrees());
        assert_eq!(*dome.orientation().consistent(), Some(true));
        assert!(dome.caveats().is_empty());
        let area = dome.measures().area().measured().expect("area");
        assert_eq!(area.reading(), None);
        assert!(area.candidates().is_empty());
    }
}

#[test]
fn a_tangent_edge_contradicts_a_flipped_dome() {
    let ir = capsule(Sense::Reversed);
    let report = measure(&ir, &Selection::default());
    let body = only_body(&report);
    let Quantity::Unavailable(volume) = body.volume().as_ref().expect("volume") else {
        panic!("the volume is unavailable");
    };
    assert_eq!(
        volume.reason(),
        UnavailableReason::VolumeOrientationConflict
    );
    let candidate = |reading: Reading| {
        volume
            .candidates()
            .iter()
            .find(|candidate| candidate.reading() == reading)
            .map(|candidate| candidate.value().value.get())
            .expect("candidate")
    };
    let cylinder = PI * RADIUS * RADIUS * LENGTH;
    let hemisphere = 2.0 * PI * RADIUS.powi(3) / 3.0;
    let scale = cylinder + 2.0 * hemisphere;
    assert!((candidate(Reading::FaceSense) - cylinder).abs() <= TEST_ABSOLUTE_FLOOR * scale);
    assert!(
        (candidate(Reading::LoopWinding) - (cylinder + 2.0 * hemisphere)).abs()
            <= TEST_ABSOLUTE_FLOOR * scale
    );
    let report = measure(&ir, &everything());
    let flipped = sphere_faces(&report)
        .into_iter()
        .find(|face| *face.orientation().consistent() == Some(false))
        .expect("a contradicted dome");
    let witness = flipped.orientation().witness().as_ref().expect("witness");
    assert_eq!(*witness.kind(), WitnessKind::TangentEdge);
    assert!(!*witness.agrees());
    assert_measured(flipped.measures().area(), 2.0 * PI * RADIUS * RADIUS, 100.0);
    let Quantity::Unavailable(bbox) = flipped.measures().bbox() else {
        panic!("the box is unavailable");
    };
    assert_eq!(bbox.reason(), UnavailableReason::RegionOrientationConflict);
    assert_eq!(bbox.candidates().len(), 2);
    assert!(!flipped
        .caveats()
        .iter()
        .any(|caveat| caveat.code() == CaveatCode::OrientationUnconfirmed));
}

#[test]
fn the_shell_sign_confirms_the_dome_of_a_hemisphere_solid() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([RADIUS, 0.0, 0.0]);
    let equator = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &vertex);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &sphere,
        Sense::Forward,
        &[vec![(equator.clone(), Sense::Forward)]],
        true,
    );
    let disk = builder.plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    builder.face(
        &disk,
        Sense::Forward,
        &[vec![(equator, Sense::Reversed)]],
        true,
    );
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &Selection::default());
    let body = only_body(&report);
    let volume = 2.0 * PI * RADIUS.powi(3) / 3.0;
    assert_measured(body.volume().as_ref().expect("volume"), volume, volume);
    let centroid = body
        .centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("centroid")
        .value()
        .value
        .get();
    assert!(centroid.x.abs() <= TEST_ABSOLUTE_FLOOR * RADIUS);
    assert!(centroid.y.abs() <= TEST_ABSOLUTE_FLOOR * RADIUS);
    assert!((centroid.z - 3.0 * RADIUS / 8.0).abs() <= TEST_ABSOLUTE_FLOOR * RADIUS);
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    let [dome] = domes.as_slice() else {
        panic!("one dome");
    };
    let witness = dome.orientation().witness().as_ref().expect("witness");
    assert_eq!(*witness.kind(), WitnessKind::ShellSign);
    assert!(*witness.agrees());
}

#[test]
fn a_hemisphere_sheet_has_one_area_under_both_readings() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([RADIUS, 0.0, 0.0]);
    let equator = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &vertex);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &sphere,
        Sense::Forward,
        &[vec![(equator, Sense::Forward)]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let domes = sphere_faces(&report);
    let [dome] = domes.as_slice() else {
        panic!("one dome");
    };
    assert_measured(dome.measures().area(), 2.0 * PI * RADIUS * RADIUS, 100.0);
    let area = dome.measures().area().measured().expect("area");
    assert_eq!(area.reading(), None);
    assert!(area.candidates().is_empty());
    let bbox = dome.measures().bbox().measured().expect("box");
    assert_eq!(bbox.reading(), Some(Reading::Declared));
    assert!((bbox.value().max.get().z - RADIUS).abs() <= TEST_ABSOLUTE_FLOOR * RADIUS);
    assert!(dome
        .caveats()
        .iter()
        .any(|caveat| caveat.code() == CaveatCode::OrientationUnconfirmed));
}
