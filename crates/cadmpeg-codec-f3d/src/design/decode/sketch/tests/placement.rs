// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn sketch_placement_decodes_compact_identity_and_explicit_affine_frame() {
    fn candidates(
        bytes: &[u8],
        scope_record_index: u32,
        entity_id: &str,
        record_index: u32,
    ) -> Vec<DesignSketchPlacement> {
        let records = IndexedRecordOffsets::build(bytes);
        parse_sketch_placement_candidates(
            bytes,
            scope_record_index,
            &crate::records::DesignEntityId::try_from(entity_id.to_owned()).expect("valid entity ID"),
            record_index,
            &records,
        )
    }

    fn placement_frame(
        record_index: u32,
        length: usize,
        transform_offset: usize,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"356");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = transform_offset + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&placement_frame(185, 201, 55, None), 177, "0_172", 185);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length(), 201);
    assert_eq!(*compact[0].transform(), identity_matrix());
    assert_eq!(compact[0].transform_offset(), None);

    let transform = [
        [0.0, 0.0, 1.0, 12.0],
        [1.0, 0.0, 0.0, 34.0],
        [0.0, 1.0, 0.0, 56.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &placement_frame(1773, 329, 55, Some(transform)),
        1765,
        "0_1761",
        1773,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length(), 329);
    assert_eq!(*explicit[0].transform(), transform);
    assert_eq!(explicit[0].transform_offset(), Some(55));

    for length in [305, 325] {
        let legacy = candidates(
            &placement_frame(1773, length, 48, Some(transform)),
            1765,
            "0_1761",
            1773,
        );
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].frame_length(), length as u64);
        assert_eq!(*legacy[0].transform(), transform);
        assert_eq!(legacy[0].transform_offset(), Some(48));
    }
}

#[test]
fn entity_genesis_placement_decodes_compact_and_explicit_frames() {
    fn candidates(
        bytes: &[u8],
        scope_record_index: u32,
        entity_id: &str,
        record_index: u32,
    ) -> Vec<DesignSketchPlacement> {
        let records = IndexedRecordOffsets::build(bytes);
        parse_sketch_placement_candidates(
            bytes,
            scope_record_index,
            &crate::records::DesignEntityId::try_from(entity_id.to_owned()).expect("valid entity ID"),
            record_index,
            &records,
        )
    }

    fn genesis_frame(
        record_index: u32,
        length: usize,
        form_byte: u8,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"293");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[55] = 1;
        bytes[65] = form_byte;
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = 66 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&genesis_frame(214, 213, 1, None), 206, "0_201", 214);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length(), 213);
    assert_eq!(*compact[0].transform(), identity_matrix());
    assert_eq!(compact[0].transform_offset(), None);

    let transform = [
        [0.0, 0.0, 1.0, 26.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &genesis_frame(3060, 341, 0, Some(transform)),
        3052,
        "0_3048",
        3060,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length(), 341);
    assert_eq!(*explicit[0].transform(), transform);
    assert_eq!(explicit[0].transform_offset(), Some(66));

    // A mismatched form byte fails both lengths.
    assert!(candidates(&genesis_frame(214, 213, 0, None), 206, "0_201", 214).is_empty());
    assert!(candidates(
        &genesis_frame(3060, 341, 1, Some(transform)),
        3052,
        "0_3048",
        3060,
    )
    .is_empty());

    // The WorkPlane sibling of this record class carries a marked record
    // reference inside the zero run and must not decode as a placement.
    let mut workplane_like = genesis_frame(214, 213, 1, None);
    workplane_like[57] = 1;
    workplane_like[58..62].copy_from_slice(&788u32.to_le_bytes());
    assert!(candidates(&workplane_like, 206, "0_201", 214).is_empty());
}

#[test]
fn entity_genesis_placement_origin_scales_to_neutral_units() {
    let placement = |form: fn(crate::records::SketchPlacementMatrix) -> crate::records::DesignSketchFrameForm| DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(0, form(crate::records::SketchPlacementMatrix::try_from([
            [0.0, 0.0, 1.0, 26.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]).unwrap())).unwrap(),

        id: "f3d:native:design-sketch-placement#0".into(),
        scope_record_index: Some(10),
        entity_id: crate::records::DesignEntityId::try_from("0_100".to_owned()).expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("293".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),

    };
    let point = SketchPoint {
        id: "f3d:native:sketch-point#0".into(),
        record_index: 20,
        owner_reference: Some(100),
        class_tag: "256".into(),
        byte_offset: 0,
        coordinate_offset: 141,
        entity_genesis: Some(2),
        record_form: crate::records::SketchPointRecordForm::version11(
            20,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(120.0, 30.0),
        depth: 0.0,
        companion: None,
    };
    let mut identityless_point = point.clone();
    identityless_point.id = "f3d:native:sketch-point#1".into();
    identityless_point.record_index = 21;
    identityless_point.coordinate_offset = 33;
    identityless_point.entity_genesis = None;
    identityless_point.record_form = crate::records::SketchPointRecordForm::Version0 { flag: 0 };

    // The `EntityGenesis`-flavor frame stores its origin in centimetres
    // while the sketch records carry ten-times-centimetre values; the
    // projected sketch origin scales by ten to stay commensurate.
    let (sketches, entities) = project_sketch_design(
        &[placement(crate::records::DesignSketchFrameForm::ScopeGenesisExplicit)],
        &[point.clone(), identityless_point],
        &[],
        &[],
        &[],
        1.0e-6,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0].resolved_placement(),
        Some((
            Point3::new(260.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == Point2::new(120.0, 30.0)
    ));
    assert_eq!(
        entities[1].id().clone(),
        crate::ids::neutral_sketch_record_id(&sketches[0].id, 21)
    );

    // The settled explicit frame keeps its stored origin unscaled.
    let (sketches, _) = project_sketch_design(&[placement(crate::records::DesignSketchFrameForm::ScopeExplicit)], &[point], &[], &[], &[], 1.0e-6);
    assert_eq!(
        sketches[0]
            .resolved_placement()
            .map(|(origin, _, _)| origin),
        Some(Point3::new(26.0, 0.0, 0.0))
    );
}

#[test]
fn feature_owned_sketch_placement_follows_member_run_head_reference() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.resize(80, 0);

    let head_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    for value in identity_matrix().into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 1]);
    bytes.resize(
        head_at + crate::design::decode::sketch::MEMBER_RUN_HEAD_FRAME,
        0,
    );

    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:design-entity-header#0".into(),
        byte_offset: 0,

        entity_id: crate::records::DesignEntityId::try_from("0_100".to_owned()).expect("valid entity ID"),
        class_tag: crate::records::DesignClassTag::try_from("281".to_owned()).unwrap(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        reference_count_present: false,
        references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        members: crate::records::ReferenceRun::Unlocated(Vec::new()),
    };
    let records = IndexedRecordOffsets::build(&bytes);
    let placement =
        crate::design::decode::sketch::parse_member_run_head_placement(&bytes, entity.byte_offset, &entity.entity_id, &records)
            .expect("feature-owned sketch placement");
    assert_eq!(placement.record_index, 200);
    assert_eq!(placement.byte_offset(), head_at as u64);
    assert_eq!(placement.paired_byte_offset(), paired_at as u64);
    assert_eq!(*placement.transform(), identity_matrix());
    assert!(placement.member_run_head());
    assert_eq!(placement.scope_record_index, None);
    assert_eq!(
        crate::design::decode::sketch::parse_legacy_sketch_container_members(
            &bytes, 0, 100, &records,
        ),
        Some(Vec::new())
    );

    bytes.truncate(head_at);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 0, 1]);
    bytes.extend_from_slice(&173u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"284");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let compact =
        crate::design::decode::sketch::parse_member_run_head_placement(&bytes, entity.byte_offset, &entity.entity_id, &records)
            .expect("compact identity sketch placement");
    assert_eq!(compact.frame_length(), 34);
    assert_eq!(*compact.transform(), identity_matrix());
    assert_eq!(compact.transform_offset(), None);
}

#[test]
fn legacy_sketch_pair_decodes_its_complete_member_run() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"381");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [300u32, 301] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let members =
        crate::design::decode::sketch::parse_legacy_sketch_member_run(&bytes, 0, 100)
            .expect("legacy sketch member run");
    assert_eq!(members.iter().map(|row| row.value).collect::<Vec<_>>(), [300, 301]);
    assert_eq!(members.iter().map(|row| row.offset).collect::<Vec<_>>(), [(paired_at + 46) as u64, (paired_at + 57) as u64]);
}

#[test]
fn legacy_line_orthogonalizes_its_auxiliary_normal() {
    let mut bytes = vec![0u8; 133];
    let values: [f64; 12] = [
        0.5,
        0.875,
        0.0,
        0.0,
        -1.75,
        0.0,
        0.0,
        -1.0,
        0.0,
        -0.000_037,
        0.000_184,
        0.999_999_982,
    ];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let SketchCurveGeometry::Line {
        direction, normal, ..
    } = crate::design::decode::sketch::decode_line(&bytes).expect("legacy line")
    else {
        panic!("expected line");
    };
    assert!((direction.norm() - 1.0).abs() <= 1.0e-12);
    assert!((normal.norm() - 1.0).abs() <= 1.0e-12);
    assert!(
        (direction.x * normal.x + direction.y * normal.y + direction.z * normal.z).abs() <= 1.0e-12
    );
    assert!(normal.z > 0.0);

    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&1.0f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes).expect("reverse-parameterized line")
    else {
        panic!("expected line");
    };
    assert!((direction.y + 1.0).abs() <= 1.0e-12);

    bytes[133 + 6 * 8..133 + 7 * 8].copy_from_slice(&0.6f64.to_le_bytes());
    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&0.8f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes)
            .expect("line with stale auxiliary direction")
    else {
        panic!("expected line");
    };
    assert!((direction.x).abs() <= 1.0e-12);
    assert!((direction.y + 1.0).abs() <= 1.0e-12);
}

#[test]
fn spatial_line_with_parallel_auxiliary_normal_retains_its_endpoints() {
    let values: [f64; 12] = [0.0, 3.0, 0.0, 0.0, 0.0, 1.5, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_line(&bytes).expect("spatial line")
    else {
        panic!("expected line");
    };
    assert_eq!(start, Point3::new(0.0, 30.0, 0.0));
    assert_eq!(end, Point3::new(0.0, 30.0, 15.0));
    assert_eq!(direction, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(normal, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn compact_planar_line_uses_its_implicit_normal() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_compact_planar_line(&bytes)
        .expect("compact planar line")
    else {
        panic!("expected line");
    };
    assert_eq!(start, Point3::new(5.0, 8.75, 0.0));
    assert_eq!(end, Point3::new(5.0, -8.75, 0.0));
    assert_eq!(direction, Vector3::new(0.0, -1.0, 0.0));
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn retained_compact_planar_line_edit_preserves_its_reference_tail() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let tail = bytes[72..].to_vec();
    let geometry = SketchCurveGeometry::Line {
        start: Point3::new(10.0, 20.0, 0.0),
        end: Point3::new(30.0, 40.0, 0.0),
        direction: Vector3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
        ),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    crate::writer::patch::records::patch_sketch_curves(
        &mut bytes,
        &[crate::writer::patch::edits::SketchCurveEdit {
            offset: 0,
            geometry_offset: 0,
            geometry,
        }],
    )
    .expect("compact planar line edit");
    assert_eq!(&bytes[72..], tail);
    assert_eq!(f64::from_le_bytes(bytes[0..8].try_into().unwrap()), 1.0);
    assert_eq!(f64::from_le_bytes(bytes[24..32].try_into().unwrap()), 2.0);
}

#[test]
fn text_frame_line_decodes_after_point_references() {
    let mut bytes = vec![0u8; 52 + 133];
    for reference in [2397u32, 2395] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        if reference == 2397 {
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"289");
    bytes.extend_from_slice(&2403u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    for value in [
        -5.75f64, 1.0, 0.0, 5.25, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let (geometry, end) = crate::design::decode::sketch::decode_text_frame_line(&bytes, 52, 2403)
        .expect("text-frame boundary line");
    assert_eq!(end, bytes.len());
    assert!(matches!(
        geometry,
        SketchCurveGeometry::Line { start, end, .. }
            if start == Point3::new(-57.5, 10.0, 0.0)
                && end == Point3::new(-5.0, 10.0, 0.0)
    ));
}

#[test]
fn legacy_sketch_nurbs_decodes_its_counted_arrays() {
    fn ascii(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let mut bytes = Vec::new();
    ascii(&mut bytes, "256");
    bytes.extend_from_slice(&1200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (name, value) in [("crv_primary_id", 700u64), ("crv_secondary_id", 0)] {
        ascii(&mut bytes, name);
        ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(bytes.len(), 133);
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&1200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0.000_01f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    marked_reference(&mut bytes, 1202);
    marked_reference(&mut bytes, 1203);
    marked_reference(&mut bytes, 1204);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e]);
    for (values, capacity) in [
        (vec![0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0], 8u32),
        (vec![1.0f64, 1.0, 1.0], 8),
        (vec![0.0f64, 0.0, 0.0, 0.5, 0.75, 0.0, 1.0, 0.0, 0.0], 8),
    ] {
        let count = u32::try_from(if values.len() == 9 {
            values.len() / 3
        } else {
            values.len()
        })
        .expect("test count");
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&capacity.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let (geometry, end) =
        crate::design::decode::sketch::decode_legacy_sketch_nurbs(&bytes).expect("legacy NURBS");
    let SketchCurveGeometry::Nurbs {
        degree,
        fit_tolerance,
        knots,
        poles,
        ..
    } = geometry
    else {
        panic!("expected NURBS");
    };
    assert_eq!(end, bytes.len());
    assert_eq!(degree, 2);
    assert_eq!(knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(poles.weights().copied().collect::<Vec<_>>(), [1.0; 3]);
    assert_eq!(poles.points().nth(1).copied().unwrap(), Point3::new(5.0, 7.5, 0.0));
    assert!((fit_tolerance - 0.000_1).abs() <= f64::EPSILON);

    marked_reference(&mut bytes, 201);
    let segment_type = |type_guid: &str, version, module: &str, entity_ids: Vec<u64>| {
        crate::records::SegmentType {
            id: String::new(),
            byte_offset: 0,
            type_guid: type_guid.into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version,
            version_offset: 0,
            module: module.into(),
            entities: crate::records::ReferenceRun::Located(entity_ids.into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect()),
        }
    };
    let meta = crate::metastream::MetaStream {
        types: vec![
            segment_type(
                "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B",
                3,
                "MSketch",
                vec![1200],
            ),
            segment_type(
                "00000000-0000-0000-0000-000000000001",
                0,
                "MSketch",
                Vec::new(),
            ),
        ],
        records: vec![crate::metastream::RecordIndexEntry {
            entity_id: 1200,
            bulk_offset: 0,
        }],
        secondary_records: vec![crate::metastream::RecordIndexEntry {
            entity_id: 1200,
            bulk_offset: 141,
        }],
    };
    let curves = crate::design::decode::sketch::decode_sketch_curve_identities_from_stream(
        &bytes,
        &meta,
        "Design/BulkStream.dat",
    )
    .expect("primary NURBS frame with a nested subtype header");
    let [curve] = curves.as_slice() else {
        panic!("one indexed NURBS curve");
    };
    assert!(matches!(
        curve.geometry,
        Some(SketchCurveGeometry::Nurbs { .. })
    ));
    assert_eq!(curve.owner_reference, Some(201));
}

#[test]
fn sketch_geometry_tail_names_its_owner_container() {
    let mut bytes = vec![0u8; 112];
    bytes.push(1);
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&400u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes[..123]),
        Some(201)
    );

    bytes[117] = 1;
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes[..123]),
        None
    );

    let mut nested = vec![0u8; 140];
    nested[120..124].copy_from_slice(&3u32.to_le_bytes());
    nested[124..127].copy_from_slice(b"302");
    nested[127..131].copy_from_slice(&500u32.to_le_bytes());
    nested.push(1);
    nested.extend_from_slice(&201u32.to_le_bytes());
    nested.extend_from_slice(&[0; 6]);
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"303");
    nested.extend_from_slice(&501u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&nested[..151]),
        Some(201)
    );
}

#[test]
fn sketch_member_run_backfills_relation_free_owners() {
    let mut bytes = vec![0u8; 40];
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 41]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let mut member_offsets = Vec::new();
    member_offsets.push((bytes.len() + 1) as u64);
    bytes.push(1);
    bytes.extend_from_slice(&99u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    for member in [20u32, 21] {
        member_offsets.push((bytes.len() + 1) as u64);
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, 0, 100),
        vec![99, 20, 21].into_iter().zip(member_offsets).map(|(value, offset)| crate::records::Located { value, offset }).collect::<Vec<_>>()
    );
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, 0, 101),
        vec![]
    );
    assert_eq!(
        crate::design::decode::sketch::parse_sketch_member_run(&bytes, paired_at + 1, 100),
        vec![]
    );

    let header = |suffix: u64, members: Vec<u32>| DesignEntityHeader {
        id: format!("f3d:native:design-entity-header#{suffix}"),
        byte_offset: suffix,

        entity_id: crate::records::DesignEntityId::try_from(format!("0_{suffix}")).expect("valid entity ID"),
        class_tag: crate::records::DesignClassTag::try_from("281".to_owned()).unwrap(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        reference_count_present: false,
        references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        members: crate::records::ReferenceRun::Located(members.into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect()),
    };
    let point = |record_index: u32| SketchPoint {
        id: format!("f3d:native:sketch-point#{record_index}"),
        record_index,
        owner_reference: None,
        class_tag: "256".into(),
        byte_offset: u64::from(record_index),
        coordinate_offset: 141,
        entity_genesis: Some(2),
        record_form: crate::records::SketchPointRecordForm::version11(
            u64::from(record_index),
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    };

    // Relation-free geometry named by the container's member run binds to
    // that sketch; records the run does not name stay unowned.
    let mut points = [point(20), point(21), point(22)];
    bind_sketch_graph(
        &[header(100, vec![20, 21, 99])],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .expect("member-run owners bind");
    assert_eq!(points[0].owner_reference, Some(100));
    assert_eq!(points[1].owner_reference, Some(100));
    assert_eq!(points[2].owner_reference, None);

    // Two sketches claiming one record is a structural conflict.
    let mut points = [point(20)];
    assert!(bind_sketch_graph(
        &[header(100, vec![20]), header(101, vec![20])],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .is_err());
}
