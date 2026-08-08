//! Tests for the `reference_geometry` module.

use super::super::curves::sketch_plane_frames;
use super::super::CLASS_MARKER;
use super::{
    angled_reference_plane_frame, classed_offset_plane_sources, compact_offset_plane_source,
    compact_reference_plane_frame, constraint_midplane_frame, constraint_reference_plane_frame,
    explicit_reference_axis_frame, explicit_reference_plane_frame, fixed_reference_plane_frame,
    legacy_offset_plane_face_alias, legacy_reference_axis_triads, matrix_reference_plane_frame,
    offset_plane_reference_frame_matches, offset_plane_reference_source,
    offset_reference_plane_frame_pair, plane_intersection_axis_frame,
    plane_intersection_axis_sources, reconcile_reference_plane_frame, reference_plane_frame_key,
    select_reference_plane_frame_source, sketch_block_identity_normalization_origin,
    sketch_block_record_origin, structured_offset_plane_sources, FIXED_REFERENCE_PLANE_FRAME_LEN,
    MINIMAL_REFERENCE_PLANE_FRAME_LEN,
};
use crate::records::Feature;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, Length, PrincipalPlane};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashSet};
#[test]
fn sketch_block_terminal_identity_carries_its_origin() {
    let mut payload = vec![0; 100];
    payload[8..12].copy_from_slice(&[0xff; 4]);
    payload[20..26].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
    payload[26..28].copy_from_slice(&17_u16.to_le_bytes());
    payload[48..52].copy_from_slice(&[0, 0, 1, 0]);
    payload[52..54].copy_from_slice(&[0x73, 0x81]);
    for (index, value) in [0.125_f64, -0.25, 0.0].into_iter().enumerate() {
        let start = 54 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(125.0, -250.0, 0.0))
    );

    payload[52..].fill(0);
    payload[52..56].copy_from_slice(CLASS_MARKER);
    payload[56..58].copy_from_slice(&17_u16.to_le_bytes());
    payload[58..75].copy_from_slice(b"moAbsolutePoint_c");
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
}

#[test]
fn sketch_block_identity_normalization_is_inverted_for_placement() {
    let mut payload = vec![0; 300];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&7_u16.to_le_bytes());
    payload.extend_from_slice(b"sgBlock");
    let body = payload.len();
    payload.resize(body + 184, 0);
    for (index, value) in [1.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let start = body + 72 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 144..body + 152].copy_from_slice(&1_u64.to_le_bytes());
    for (index, value) in [-0.21_f64, 0.661, 0.0].into_iter().enumerate() {
        let start = body + 152 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 176..body + 184].copy_from_slice(&1.0_f64.to_le_bytes());

    assert_eq!(
        sketch_block_identity_normalization_origin(&payload, 200, payload.len()),
        Some(Point3::new(210.0, -661.0, 0.0))
    );
}

#[test]
fn plane_intersection_axis_requires_two_complete_known_references() {
    let record = |source: u32, object: u8, selector: u8| {
        let mut bytes = vec![0; 46];
        bytes[..4].copy_from_slice(&source.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x6255_5715u32.to_le_bytes());
        bytes[14..16].copy_from_slice(&[1, 0]);
        bytes[22] = object;
        bytes[30] = selector;
        bytes[38..46].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let mut payload = record(17, 0xb6, 3);
    payload.extend_from_slice(&record(23, 0x98, 0));
    let known = [17, 23].into_iter().collect();
    assert_eq!(
        plane_intersection_axis_sources(&payload, &known),
        Some([17, 23])
    );

    payload.pop();
    assert_eq!(plane_intersection_axis_sources(&payload, &known), None);
    let incomplete = record(17, 0xb6, 3);
    assert_eq!(plane_intersection_axis_sources(&incomplete, &known), None);
}

#[test]
fn legacy_reference_axis_triad_requires_consecutive_native_records() {
    let feature = |ordinal: u32, source: u32, class: &str| Feature {
        id: format!("feature-{ordinal}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.to_string()),
        parent_source_id: None,
        ordinal,
        name: String::new(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut features = (0..3)
        .map(|index| feature(10 + index, 40 + index, "moRefPlane_c"))
        .chain((0..3).map(|index| feature(13 + index, 43 + index, "moRefAxis_c")))
        .collect::<Vec<_>>();
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([3, 4, 5], [[40, 41], [40, 42], [42, 41]])]
    );

    features.insert(3, feature(99, 4, "moRefPlane_c"));
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([4, 5, 6], [[40, 41], [40, 42], [42, 41]])]
    );

    features[5].source_id = Some("99".into());
    assert!(legacy_reference_axis_triads(&features).is_empty());
}

#[test]
fn plane_intersection_axis_uses_the_closest_point_to_the_origin() {
    let first = (
        Point3::new(2.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    let second = (
        Point3::new(0.0, -3.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert_eq!(
        plane_intersection_axis_frame(first, second),
        Some((Point3::new(2.0, -3.0, 0.0), Vector3::new(0.0, 0.0, 1.0),))
    );

    let parallel = (
        Point3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(plane_intersection_axis_frame(first, parallel), None);
}

#[test]
fn explicit_reference_axis_requires_redundant_collinear_witnesses() {
    let mut record = vec![0; 88];
    for (offset, value) in [
        (0, 0.25_f64),
        (8, -0.4),
        (16, 0.1),
        (24, 0.25),
        (32, 0.6),
        (40, 0.1),
        (48, 0.0),
        (56, -0.5),
        (64, 0.0),
        (72, 1.0),
        (80, 0.0),
    ] {
        record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = vec![0xaa; 17];
    payload.extend_from_slice(&record);
    payload.extend_from_slice(&[0xbb; 11]);
    assert_eq!(
        explicit_reference_axis_frame(&payload),
        Some((Point3::new(250.0, 0.0, 100.0), Vector3::new(0.0, 1.0, 0.0),))
    );

    record[24..32].copy_from_slice(&0.5_f64.to_le_bytes());
    assert_eq!(explicit_reference_axis_frame(&record), None);
}

#[test]
fn fixed_reference_plane_uses_all_three_stored_basis_vectors() {
    let mut frame = [0; FIXED_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.374_f64),
        (8, -0.25),
        (16, 0.125),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = 1;
    assert_eq!(
        fixed_reference_plane_frame(&frame),
        Some((
            Point3::new(374.0, -250.0, 125.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(fixed_reference_plane_frame(&frame), None);
    assert_eq!(fixed_reference_plane_frame(&frame[..96]), None);
}

#[test]
fn reference_plane_frame_identity_canonicalizes_signed_zero() {
    let positive = (
        Point3::new(0.0, 1.0, 2.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let negative = (
        Point3::new(-0.0, 1.0, 2.0),
        Vector3::new(1.0, -0.0, 0.0),
        Vector3::new(0.0, -0.0, 1.0),
    );

    assert_eq!(
        reference_plane_frame_key(&positive),
        reference_plane_frame_key(&negative)
    );
}

#[test]
fn offset_plane_frame_pair_stores_result_before_reference() {
    let frame = |origin_x: f64| {
        let mut bytes = [0; FIXED_REFERENCE_PLANE_FRAME_LEN];
        for (offset, value) in [
            (0, origin_x / 1000.0),
            (8, 0.0),
            (16, 0.0),
            (24, 1.0),
            (32, 0.0),
            (40, 0.0),
            (49, 0.0),
            (57, 0.0),
            (65, 1.0),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(-37.0).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 37.0),
        Some((
            (
                Point3::new(-37.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
        ))
    );
    payload[65..73].copy_from_slice(&(-1.0_f64).to_le_bytes());
    assert!(offset_reference_plane_frame_pair(&payload, 37.0).is_some());
    assert_eq!(offset_reference_plane_frame_pair(&payload, 38.0), None);

    let mut antiparallel = frame(-37.0).to_vec();
    antiparallel[24..32].copy_from_slice(&(-1.0_f64).to_le_bytes());
    antiparallel.extend([0; 13]);
    antiparallel.extend(frame(0.0));
    assert!(offset_reference_plane_frame_pair(&antiparallel, 37.0).is_some());
}

#[test]
fn offset_plane_frame_pair_accepts_ordered_mixed_frame_layouts() {
    let mut result = [0; MINIMAL_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.210),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -0.210),
        (73, 1.0),
    ] {
        result[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    result[56] = 0x80;
    let mut reference = [0; 82];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.235),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        reference[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = result.to_vec();
    payload.extend([0xff; 19]);
    payload.extend(reference);

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 25.0),
        Some((
            (
                Point3::new(0.0, 0.0, 210.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
            (
                Point3::new(0.0, 0.0, 235.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ))
    );
}

#[test]
fn tangent_plane_frame_is_anchored_to_its_constraint_class() {
    const CLASS: &str = "moConstraintPerpPlnTanOneCylinderRefplaneData_c";
    let root = 7;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + FIXED_REFERENCE_PLANE_FRAME_LEN, 0);
    for (relative, value) in [
        (0, 0.0125_f64),
        (24, 1.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(12.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, "moRefPlane_c"),
        None
    );
}

#[test]
fn offset_plane_face_reference_owns_a_fixed_plane_frame() {
    const CLASS: &str = "moFaceRefPlnData_c";
    let root = 11;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + FIXED_REFERENCE_PLANE_FRAME_LEN, 0);
    for (relative, value) in [(0, 0.0025_f64), (24, 1.0), (57, 1.0), (89, 1.0)] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(2.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
}

#[test]
fn offset_plane_reference_matches_parallel_frame_at_declared_distance() {
    let reference = (
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    let offset = (
        Point3::new(0.0, 0.0, 6.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert!(offset_plane_reference_frame_matches(reference, offset, 6.0));
    assert!(!offset_plane_reference_frame_matches(
        reference, offset, 5.0
    ));
    assert!(!offset_plane_reference_frame_matches(
        reference,
        (Point3::new(1.0, 0.0, 6.0), offset.1, offset.2,),
        6.0,
    ));
}

#[test]
fn constraint_midplane_uses_its_normal_form_equation() {
    const CLASS: &str = "moConstraintMidPlaneRefplaneData_c";
    let mut payload = vec![0xaa; 19];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    payload.extend([0; 8]);
    payload.extend(1.0e-16f64.to_le_bytes());
    payload.extend(0.145f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(1.0f64.to_le_bytes());
    assert_eq!(
        constraint_midplane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, 145.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    let normal = payload.len() - 24;
    payload[normal..normal + 8].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(constraint_midplane_frame(&payload), None);
}

#[test]
fn explicit_plane_basis_precedes_equivalent_constraint_orientation() {
    let explicit = (
        Point3::new(12.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let equivalent_constraint = (
        Point3::new(12.0, 4.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(
        reconcile_reference_plane_frame(Some(explicit), Some(equivalent_constraint)),
        Some(explicit)
    );

    let conflicting_constraint = (
        Point3::new(13.0, 0.0, 0.0),
        equivalent_constraint.1,
        equivalent_constraint.2,
    );
    assert_eq!(
        reconcile_reference_plane_frame(Some(explicit), Some(conflicting_constraint)),
        Some(conflicting_constraint)
    );
}

#[test]
fn angled_reference_plane_requires_its_redundant_normal_and_basis() {
    let root = 11;
    let mut payload = vec![0; root + 121];
    let inverse_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    for (relative, value) in [
        (0, inverse_sqrt_two),
        (8, inverse_sqrt_two),
        (17, 1.0),
        (25, 0.0),
        (33, 0.0),
        (41, 0.0),
        (49, inverse_sqrt_two),
        (57, inverse_sqrt_two),
        (65, 0.0),
        (73, -inverse_sqrt_two),
        (81, inverse_sqrt_two),
        (113, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 16] = 1;
    assert_eq!(
        angled_reference_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, inverse_sqrt_two, inverse_sqrt_two),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 8..root + 16].copy_from_slice(&(-inverse_sqrt_two).to_le_bytes());
    assert_eq!(angled_reference_plane_frame(&payload), None);
}

#[test]
fn angled_reference_plane_does_not_reinterpret_a_complete_fixed_frame() {
    let mut payload = vec![0; 153];
    for (offset, value) in [
        (24, 0.0_f64),
        (32, -1.0),
        (40, 0.0),
        (49, -1.0),
        (57, 0.0),
        (65, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, -1.0),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
        (145, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[48] = 1;
    assert!(fixed_reference_plane_frame(&payload[..97]).is_some());
    assert_eq!(angled_reference_plane_frame(&payload), None);
}

#[test]
fn matrix_reference_plane_uses_basis_columns() {
    let root = 9;
    let mut payload = vec![0; root + 121];
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    for (relative, value) in [
        (0, 0.008_400_719_262_519_38),
        (8, 0.019_790_854_349_227_484),
        (16, 0.0),
        (24, sine),
        (32, cosine),
        (40, 0.0),
        (49, cosine),
        (57, 0.0),
        (65, sine),
        (73, -sine),
        (81, 0.0),
        (89, cosine),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 48] = 1;
    assert_eq!(
        matrix_reference_plane_frame(&payload),
        Some((
            Point3::new(
                0.008_400_719_262_519_38 * 1000.0,
                0.019_790_854_349_227_484 * 1000.0,
                0.0,
            ),
            Vector3::new(sine, cosine, 0.0),
            Vector3::new(cosine, -sine, 0.0),
        ))
    );

    payload[root + 113..root + 121].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(matrix_reference_plane_frame(&payload), None);
}

#[test]
fn complete_reference_plane_frames_precede_compact_byte_patterns() {
    let mut payload = vec![0; 260];
    let matrix = 3;
    for (relative, value) in [
        (0, 0.035_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
        (97, -1.0),
        (105, 0.0),
        (113, 0.0),
    ] {
        payload[matrix + relative..matrix + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[matrix + 48] = 1;

    let compact = 165;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[compact + relative..compact + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[compact + 64] = 0;
    payload[compact + 81] = 0;

    assert!(compact_reference_plane_frame(&payload).is_some());
    assert_eq!(
        explicit_reference_plane_frame(&payload),
        Ok(Some((
            Point3::new(35.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        )))
    );
}

#[test]
fn compact_reference_plane_solves_omitted_basis_components() {
    let root = 7;
    let mut payload = vec![0xaa; root + 82];
    for (relative, value) in [
        (0, 0.001_f64),
        (8, -0.002),
        (16, 0.003),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 64] = 0;
    payload[root + 81] = 0;
    assert_eq!(
        compact_reference_plane_frame(&payload),
        Some((
            Point3::new(1.0, -2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 73..root + 81].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(compact_reference_plane_frame(&payload), None);
}

#[test]
fn compact_offset_plane_source_requires_the_reference_record() {
    let mut payload = Vec::new();
    payload.extend(3u32.to_le_bytes());
    payload.extend([
        0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x2d, 0x80, 0x2b, 0x80,
    ]);
    assert_eq!(compact_offset_plane_source(&payload), Some(3));
    payload[19] ^= 1;
    assert_eq!(compact_offset_plane_source(&payload), None);
}

#[test]
fn legacy_offset_plane_face_alias_requires_the_complete_nested_record() {
    let mut body = vec![0; 115];
    body[..2].copy_from_slice(&0x802d_u16.to_le_bytes());
    body[2..6].copy_from_slice(&2u32.to_le_bytes());
    body[45..61].fill(0xff);
    body[69..73].copy_from_slice(&2u32.to_le_bytes());
    body[73..77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
    body[77..83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
    body[83..87].copy_from_slice(&1u32.to_le_bytes());
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[99..103].copy_from_slice(&3u32.to_le_bytes());
    body[107..115].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(legacy_offset_plane_face_alias(&body), Some((0, 175)));
    body[91..95].fill(0);
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[83] = 2;
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
}

#[test]
fn structured_offset_plane_source_requires_repeated_identities_and_terminator() {
    let mut payload = vec![0; 140];
    let header = 0x8323u32.to_le_bytes();
    let identity = [
        0xd7, 0x81, 0x26, 0x03, 0x1d, 0x00, 0x00, 0x00, 0x5e, 0x2c, 0xdb, 0x54,
    ];
    let link = 0x81dcu32.to_le_bytes();
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&header);
    for offset in [8, 32, 52, 76] {
        payload[offset..offset + 12].copy_from_slice(&identity);
    }
    payload[28..32].copy_from_slice(&link);
    payload[44..48].copy_from_slice(&3u32.to_le_bytes());
    payload[48..52].copy_from_slice(&header);
    for offset in [64, 88, 108] {
        payload[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[72..76].copy_from_slice(&link);
    payload[116..120].copy_from_slice(&2600u32.to_le_bytes());
    payload[132..140].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(structured_offset_plane_sources(&payload), [3]);
    payload[80] ^= 1;
    assert!(structured_offset_plane_sources(&payload).is_empty());
}

#[test]
fn classed_offset_plane_source_requires_exact_length_delimited_type() {
    let mut payload = 4u32.to_le_bytes().to_vec();
    payload.extend(b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00");

    assert_eq!(classed_offset_plane_sources(&payload), [4]);
    payload[8] = 0;
    assert!(classed_offset_plane_sources(&payload).is_empty());
}

#[test]
fn typed_offset_plane_reference_requires_one_known_plane_target() {
    let record = |source: u32, signature: [u8; 4], selector: u32| {
        let mut bytes = Vec::new();
        bytes.extend(source.to_le_bytes());
        bytes.extend(signature);
        bytes.extend([0; 2]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend(247u32.to_le_bytes());
        bytes.extend([0; 12]);
        bytes.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let known = HashSet::from([3, 225]);
    let principal = record(3, [0x43, 0xf6, 0x8a, 0x4d], 3);
    assert_eq!(
        offset_plane_reference_source(&principal, &known, &known, None),
        Some(3)
    );
    let feature = record(225, [0x30, 0x92, 0xab, 0x53], 0);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, None),
        Some(225)
    );
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, Some(225)),
        None
    );

    let mut ambiguous = principal.clone();
    ambiguous.extend_from_slice(&feature);
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        None
    );
    let mut repeated = principal.clone();
    repeated.extend_from_slice(&principal);
    assert_eq!(
        offset_plane_reference_source(&repeated, &known, &known, None),
        Some(3)
    );
    ambiguous[38] ^= 1;
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        Some(225)
    );
    let mut malformed = record(3, [0; 4], 2);
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        None
    );
    malformed[4..8].copy_from_slice(&[1, 2, 3, 4]);
    malformed[10..14].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        Some(3)
    );
    let principal_only = HashSet::from([3]);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &principal_only, None),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_requires_one_unique_source() {
    assert_eq!(
        select_reference_plane_frame_source(["derived", "principal", "older"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["same", "same"].into_iter()),
        Some("same".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_does_not_use_feature_order() {
    assert_eq!(
        select_reference_plane_frame_source(["older", "latest", "latest"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["source", "source"].into_iter()),
        Some("source".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn offset_plane_frame_translates_its_reference_frame() {
    use cadmpeg_ir::features::Feature as NeutralFeature;

    let native = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let neutral = |id: &str, native_ref: &str, definition| NeutralFeature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let features = vec![
        neutral(
            "plane",
            "plane-native",
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
        ),
        neutral(
            "offset",
            "offset-native",
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(
                    FeatureId("plane".into()),
                )),
                distance: Length(3.0),
            },
        ),
    ];
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("plane-native", "3"), native("offset-native", "549")],
    };

    assert_eq!(
        sketch_plane_frames(&features, &[history]).get(&549),
        Some(&(
            Point3::new(0.0, 0.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ))
    );
}
