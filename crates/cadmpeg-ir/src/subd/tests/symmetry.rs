// SPDX-License-Identifier: Apache-2.0

use crate::math::{Point3, Vector3};
use crate::subd::{SubdPlaneFrame, SubdSymmetry, SubdSymmetryKind, EPS_SUBD_SYMMETRY_FRAME};

fn plane() -> SubdPlaneFrame {
    SubdPlaneFrame::new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
    .unwrap()
}

#[test]
fn plane_frame_admission_rejects_non_finite_and_non_orthonormal_components() {
    for (origin, first, second) in [
        (
            Point3::new(f64::INFINITY, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
        (
            plane().origin(),
            Vector3::new(f64::NAN, 0.0, 0.0),
            plane().second_axis(),
        ),
        (
            plane().origin(),
            plane().first_axis(),
            Vector3::new(0.0, f64::INFINITY, 0.0),
        ),
        (
            plane().origin(),
            Vector3::new(0.0, 0.0, 0.0),
            plane().second_axis(),
        ),
        (
            plane().origin(),
            plane().first_axis(),
            Vector3::new(0.0, 2.0, 0.0),
        ),
        (plane().origin(), plane().first_axis(), plane().first_axis()),
    ] {
        assert!(SubdPlaneFrame::new(origin, first, second).is_err());
        let wire =
            serde_json::json!({"origin": origin, "first_axis": first, "second_axis": second});
        assert!(serde_json::from_value::<SubdPlaneFrame>(wire).is_err());
    }
}

#[test]
fn plane_frame_admission_preserves_the_unit_and_orthogonality_tolerance() {
    let eps = EPS_SUBD_SYMMETRY_FRAME;
    for (first, second) in [
        (
            Vector3::new(1.0 + eps * 0.5, 0.0, 0.0),
            plane().second_axis(),
        ),
        (plane().first_axis(), Vector3::new(eps * 0.5, 1.0, 0.0)),
    ] {
        let frame = SubdPlaneFrame::new(plane().origin(), first, second).unwrap();
        assert_eq!(frame.first_axis(), first);
        assert_eq!(frame.second_axis(), second);
        assert_eq!(
            serde_json::from_value::<SubdPlaneFrame>(serde_json::to_value(frame).unwrap()).unwrap(),
            frame
        );
    }
    assert!(SubdPlaneFrame::new(
        plane().origin(),
        Vector3::new(1.0 + eps * 2.0, 0.0, 0.0),
        plane().second_axis()
    )
    .is_err());
    assert!(SubdPlaneFrame::new(
        plane().origin(),
        plane().first_axis(),
        Vector3::new(eps * 2.0, 1.0, 0.0)
    )
    .is_err());
}

fn radial(sweep: f64) -> Result<SubdSymmetry, crate::subd::SubdError> {
    SubdSymmetry::new(
        SubdSymmetryKind::Radial {
            segments: std::num::NonZeroU32::new(1).unwrap(),
            sweep,
            radial_maps: Vec::new(),
        },
        plane(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn radial_controls_require_nonzero_segments_and_finite_sweeps() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(radial(invalid).is_err());
    }
    let mut wire = serde_json::to_value(radial(1.0).unwrap()).unwrap();
    wire["kind"]["segments"] = 0.into();
    assert!(serde_json::from_value::<SubdSymmetry>(wire)
        .unwrap_err()
        .to_string()
        .contains("segments"));
    for sweep in [-f64::MAX, -1.0, 0.0, f64::MAX] {
        let symmetry = radial(sweep).unwrap();
        assert!(
            matches!(symmetry.kind(), SubdSymmetryKind::Radial { sweep: actual, .. } if *actual == sweep)
        );
        assert_eq!(
            serde_json::from_value::<SubdSymmetry>(serde_json::to_value(&symmetry).unwrap())
                .unwrap(),
            symmetry
        );
    }
}
