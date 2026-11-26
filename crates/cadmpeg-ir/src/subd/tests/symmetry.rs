// SPDX-License-Identifier: Apache-2.0

use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::subd::{
    SubdPlaneFrame, SubdRadialMapSelector, SubdRadialSymmetry, SubdRadialSymmetryMap, SubdSymmetry,
    SubdSymmetryKind, EPS_SUBD_SYMMETRY_FRAME,
};
use cadmpeg_core::serde_erase::json;

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
            plane().origin.get(),
            Vector3::new(f64::NAN, 0.0, 0.0),
            *plane().second_axis.as_raw(),
        ),
        (
            plane().origin.get(),
            *plane().first_axis.as_raw(),
            Vector3::new(0.0, f64::INFINITY, 0.0),
        ),
        (
            plane().origin.get(),
            Vector3::new(0.0, 0.0, 0.0),
            *plane().second_axis.as_raw(),
        ),
        (
            plane().origin.get(),
            *plane().first_axis.as_raw(),
            Vector3::new(0.0, 2.0, 0.0),
        ),
        (
            plane().origin.get(),
            *plane().first_axis.as_raw(),
            *plane().first_axis.as_raw(),
        ),
    ] {
        assert!(SubdPlaneFrame::new(origin, first, second).is_err());
        let wire =
            serde_json::json!({"origin": origin, "first_axis": first, "second_axis": second});
        assert!(json::from_value::<SubdPlaneFrame>(wire).is_err());
    }
}

#[test]
fn plane_frame_admission_preserves_the_unit_and_orthogonality_tolerance() {
    let eps = EPS_SUBD_SYMMETRY_FRAME;
    for (first, second) in [
        (
            Vector3::new(1.0 + eps * 0.5, 0.0, 0.0),
            *plane().second_axis.as_raw(),
        ),
        (
            *plane().first_axis.as_raw(),
            Vector3::new(eps * 0.5, 1.0, 0.0),
        ),
    ] {
        let frame = SubdPlaneFrame::new(plane().origin.get(), first, second).unwrap();
        assert_eq!(*frame.first_axis.as_raw(), first);
        assert_eq!(*frame.second_axis.as_raw(), second);
        assert_eq!(
            json::from_value::<SubdPlaneFrame>(json::to_value(frame).unwrap()).unwrap(),
            frame
        );
    }
    assert!(SubdPlaneFrame::new(
        plane().origin.get(),
        Vector3::new(1.0 + eps * 2.0, 0.0, 0.0),
        *plane().second_axis.as_raw()
    )
    .is_err());
    assert!(SubdPlaneFrame::new(
        plane().origin.get(),
        *plane().first_axis.as_raw(),
        Vector3::new(eps * 2.0, 1.0, 0.0)
    )
    .is_err());
}

fn radial_kind(sweep: f64, radial_maps: Vec<SubdRadialSymmetryMap>) -> SubdSymmetryKind {
    SubdSymmetryKind::Radial(SubdRadialSymmetry {
        segments: std::num::NonZeroU32::new(1).unwrap(),
        sweep: FiniteReal::new(sweep).unwrap(),
        radial_maps,
    })
}

fn radial(sweep: f64) -> Result<SubdSymmetry, crate::subd::SubdError> {
    SubdSymmetry::new(
        radial_kind(sweep, Vec::new()),
        plane(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn radial_controls_require_nonzero_segments_and_keep_finite_sweeps() {
    let mut wire = json::to_value(radial(1.0).unwrap()).unwrap();
    wire["kind"]["segments"] = 0.into();
    assert!(json::from_value::<SubdSymmetry>(wire)
        .unwrap_err()
        .to_string()
        .contains("nonzero"));
    for sweep in [-f64::MAX, -1.0, 0.0, f64::MAX] {
        let symmetry = radial(sweep).unwrap();
        assert!(
            matches!(&symmetry.kind, SubdSymmetryKind::Radial(radial) if radial.sweep.get() == sweep)
        );
        assert_eq!(
            json::from_value::<SubdSymmetry>(json::to_value(&symmetry).unwrap()).unwrap(),
            symmetry
        );
    }
}

/// A radial sweep is a `FiniteReal`, so a non-finite sweep does not
/// deserialize: neither the raw value nor the `null` that JSON writes for it.
#[test]
fn radial_sweep_refuses_non_finite_values() {
    use serde::de::value::{Error, F64Deserializer};
    for sweep in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            <FiniteReal as serde::Deserialize>::deserialize(F64Deserializer::<Error>::new(sweep))
                .is_err()
        );
        let mut wire = json::to_value(radial(1.0).unwrap()).unwrap();
        wire["kind"]["sweep"] = serde_json::json!(sweep);
        assert!(json::from_value::<SubdSymmetry>(wire).is_err());
    }
}

#[test]
fn symmetry_pair_admission_requires_distinct_sources_and_targets() {
    for invalid in [vec![[0, 0], [0, 1]], vec![[0, 1], [1, 1]]] {
        for (index, field) in ["face_pairs", "edge_pairs", "vertex_pairs"]
            .into_iter()
            .enumerate()
        {
            let mut pairs = [Vec::new(), Vec::new(), Vec::new()];
            pairs[index] = invalid.clone();
            let [faces, edges, vertices] = pairs;
            assert!(SubdSymmetry::new(
                SubdSymmetryKind::Correspondence {},
                plane(),
                faces,
                edges,
                vertices,
            )
            .unwrap_err()
            .to_string()
            .contains(field));
            let symmetry = SubdSymmetry::new(
                SubdSymmetryKind::Correspondence {},
                plane(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
            let mut wire = json::to_value(symmetry).unwrap();
            wire[field] = serde_json::json!(invalid);
            assert!(json::from_value::<SubdSymmetry>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
    let pairs = vec![[0, 0], [u32::MAX, u32::MAX]];
    let symmetry = SubdSymmetry::new(
        SubdSymmetryKind::Correspondence {},
        plane(),
        pairs.clone(),
        pairs.clone(),
        pairs.clone(),
    )
    .unwrap();
    assert_eq!(symmetry.face_pairs, pairs);
    assert_eq!(symmetry.edge_pairs, pairs);
    assert_eq!(symmetry.vertex_pairs, pairs);
    assert_eq!(
        json::from_value::<SubdSymmetry>(json::to_value(&symmetry).unwrap()).unwrap(),
        symmetry
    );
}

/// A radial map is the source's native map, kept for its writer, so a
/// repeated selector or source reads and is kept.
#[test]
fn radial_maps_keep_repeated_selectors_and_sources() {
    let map = |pairs| SubdRadialSymmetryMap {
        selector: SubdRadialMapSelector::Ef,
        pairs,
    };
    for maps in [
        vec![map(vec![]), map(vec![])],
        vec![map(vec![[0, 0], [0, 1]])],
    ] {
        let kind = radial_kind(0.0, maps.clone());
        assert!(matches!(&kind, SubdSymmetryKind::Radial(radial) if radial.radial_maps == maps));
        let mut wire = json::to_value(radial(0.0).unwrap()).unwrap();
        wire["kind"]["radial_maps"] = json::to_value(&maps).unwrap();
        let read = json::from_value::<SubdSymmetry>(wire.clone()).unwrap();
        assert_eq!(json::to_value(read).unwrap(), wire);
    }
    let symmetry = SubdSymmetry::new(
        radial_kind(0.0, vec![map(vec![[0, 0], [u64::MAX, 0]])]),
        plane(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        json::from_value::<SubdSymmetry>(json::to_value(&symmetry).unwrap()).unwrap(),
        symmetry
    );
}

#[test]
fn radial_kind_deserialization_admits_controls_without_a_symmetry_owner() {
    let wire = serde_json::json!({
        "kind": "radial", "segments": 4, "sweep": 1.0,
        "radial_maps": [{ "selector": "ef", "pairs": [[0, 1]] }]
    });
    let kind = json::from_value::<SubdSymmetryKind>(wire.clone()).unwrap();
    assert_eq!(json::to_value(kind).unwrap(), wire);
    let mut zero_segments = wire.clone();
    zero_segments["segments"] = 0.into();
    assert!(json::from_value::<SubdSymmetryKind>(zero_segments).is_err());
    let mut duplicate_sources = wire.clone();
    duplicate_sources["radial_maps"][0]["pairs"] = serde_json::json!([[0, 1], [0, 2]]);
    assert!(json::from_value::<SubdSymmetryKind>(duplicate_sources).is_ok());
    let mut duplicate_selectors = wire.clone();
    duplicate_selectors["radial_maps"] =
        serde_json::json!([wire["radial_maps"][0], wire["radial_maps"][0]]);
    assert!(json::from_value::<SubdSymmetryKind>(duplicate_selectors).is_ok());
}
