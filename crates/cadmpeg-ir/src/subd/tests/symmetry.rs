// SPDX-License-Identifier: Apache-2.0

use crate::math::{Point3, Vector3};
use crate::subd::{
    SubdPlaneFrame, SubdRadialMapSelector, SubdRadialSymmetryMap, SubdSymmetry, SubdSymmetryKind,
    EPS_SUBD_SYMMETRY_FRAME,
};

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
        SubdSymmetryKind::radial(std::num::NonZeroU32::new(1).unwrap(), sweep, Vec::new())?,
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
        .contains("nonzero"));
    for sweep in [-f64::MAX, -1.0, 0.0, f64::MAX] {
        let symmetry = radial(sweep).unwrap();
        assert!(
            matches!(symmetry.kind(), SubdSymmetryKind::Radial(radial) if radial.sweep() == sweep)
        );
        assert_eq!(
            serde_json::from_value::<SubdSymmetry>(serde_json::to_value(&symmetry).unwrap())
                .unwrap(),
            symmetry
        );
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
                vertices
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
            let mut wire = serde_json::to_value(symmetry).unwrap();
            wire[field] = serde_json::json!(invalid);
            assert!(serde_json::from_value::<SubdSymmetry>(wire)
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
    assert_eq!(symmetry.face_pairs(), pairs);
    assert_eq!(symmetry.edge_pairs(), pairs);
    assert_eq!(symmetry.vertex_pairs(), pairs);
    assert_eq!(
        serde_json::from_value::<SubdSymmetry>(serde_json::to_value(&symmetry).unwrap()).unwrap(),
        symmetry
    );
}

#[test]
fn radial_maps_require_distinct_selectors_and_sources() {
    let map = |pairs| SubdRadialSymmetryMap {
        selector: SubdRadialMapSelector::Ef,
        pairs,
    };
    for maps in [
        vec![map(vec![]), map(vec![])],
        vec![map(vec![[0, 0], [0, 1]])],
    ] {
        assert!(
            SubdSymmetryKind::radial(std::num::NonZeroU32::new(1).unwrap(), 0.0, maps.clone())
                .is_err()
        );
        let mut wire = serde_json::to_value(radial(0.0).unwrap()).unwrap();
        wire["kind"]["radial_maps"] = serde_json::to_value(maps).unwrap();
        assert!(serde_json::from_value::<SubdSymmetry>(wire).is_err());
    }
    let symmetry = SubdSymmetry::new(
        SubdSymmetryKind::radial(
            std::num::NonZeroU32::new(1).unwrap(),
            0.0,
            vec![map(vec![[0, 0], [u64::MAX, 0]])],
        )
        .unwrap(),
        plane(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<SubdSymmetry>(serde_json::to_value(&symmetry).unwrap()).unwrap(),
        symmetry
    );
}

#[test]
fn radial_kind_deserialization_admits_controls_without_a_symmetry_owner() {
    let wire = serde_json::json!({
        "kind": "radial", "segments": 4, "sweep": 1.0,
        "radial_maps": [{ "selector": "ef", "pairs": [[0, 1]] }]
    });
    let kind = serde_json::from_value::<SubdSymmetryKind>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(kind).unwrap(), wire);
    let mut zero_segments = wire.clone();
    zero_segments["segments"] = 0.into();
    assert!(serde_json::from_value::<SubdSymmetryKind>(zero_segments).is_err());
    let mut duplicate_sources = wire.clone();
    duplicate_sources["radial_maps"][0]["pairs"] = serde_json::json!([[0, 1], [0, 2]]);
    assert!(serde_json::from_value::<SubdSymmetryKind>(duplicate_sources).is_err());
    let mut duplicate_selectors = wire.clone();
    duplicate_selectors["radial_maps"] =
        serde_json::json!([wire["radial_maps"][0], wire["radial_maps"][0]]);
    assert!(serde_json::from_value::<SubdSymmetryKind>(duplicate_selectors).is_err());
}
