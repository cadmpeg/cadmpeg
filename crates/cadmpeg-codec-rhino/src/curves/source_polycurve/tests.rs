// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{IdentityKey, ProceduralCurveId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::SourceFidelity;

use crate::chunks::ArchiveVersion;
use crate::curves::{DecodedCurve, DecodedGeometry, GeometryError};
use crate::settings::MillimeterScale;
use crate::test_support::test_archive;

fn children() -> Vec<([u8; 16], Vec<u8>)> {
    vec![
        (
            test_archive::LINE_CLASS,
            test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        ),
        (
            test_archive::LINE_CLASS,
            test_archive::line_payload([1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0]),
        ),
    ]
}

fn assert_tree(decoded: &DecodedGeometry) {
    let DecodedGeometry::Curve {
        curve:
            DecodedCurve::Compound {
                children,
                end_parameter,
                ..
            },
    } = decoded
    else {
        panic!("compound construction")
    };
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].0.get(), -2.0);
    assert_eq!(children[1].0.get(), 1.0);
    assert_eq!(end_parameter.get(), 6.0);
    for ((_, child), (start, end)) in children.iter().zip([
        (Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
        (Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)),
    ]) {
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = child.reported_geometry()
        else {
            panic!("unchanged native line carrier")
        };
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(
            nurbs.control_points().first().expect("first pole").get(),
            start
        );
        assert_eq!(nurbs.control_points().last().expect("last pole").get(), end);
        assert_eq!(
            child
                .parameter_range()
                .expect("native line domain")
                .endpoints(),
            [0.0, 1.0]
        );
    }
}

#[test]
fn nonzero_polycurve_source_window_keeps_parameters_and_skips_reserved_fields_and_leaf_bytes() {
    let fixture = test_archive::polycurve_source(&[-2.0, 1.0, 6.0], &children());
    let mut bytes = b"prefix!".to_vec();
    let start = bytes.len();
    bytes.extend(&fixture.bytes);
    let fields_end = bytes.len();
    bytes.extend(b"unparsed-extension");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("genuine source");
    let source = root
        .child(start, bytes.len())
        .expect("nonzero source window");
    let key = IdentityKey::try_new("source".to_owned()).expect("source key");
    let decoded = super::decode(
        &ctx,
        source,
        &bytes,
        start..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &key,
    )
    .expect("source polycurve");
    assert_tree(&decoded);
    let detached = super::decode(
        &ctx,
        View::over_retained(&bytes),
        &bytes,
        start..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &key,
    )
    .expect("same detached grammar");
    assert_tree(&detached);
    let id = ProceduralCurveId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
        key,
    );
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("one extraction");
    assert_eq!(fidelity.record_layout().len(), 3);
    let layout = &fidelity.record_layout()[id.as_str()];
    assert_eq!(layout.offset(), u64::try_from(start).expect("small start"));
    assert_eq!(
        layout.byte_len(),
        u64::try_from(bytes.len() - start).expect("small extent")
    );
    assert_eq!(
        layout.reads()["file"],
        [
            u64::try_from(start).expect("small start")
                ..u64::try_from(start + fixture.reserved.start).expect("small reserved start"),
            u64::try_from(start + fixture.parameter_array.start).expect("small array start")
                ..u64::try_from(start + fixture.parameter_array.end).expect("small array end"),
        ]
    );
    let mut cursor = start + fixture.parameter_array.end;
    for index in 0..2 {
        let wrapper =
            crate::chunks::chunk_at(&bytes, cursor, bytes.len(), ArchiveVersion::V5, false)
                .expect("actual line wrapper");
        let child = crate::objects::parse_class_wrapper(
            &bytes,
            cursor..wrapper.next_offset(),
            ArchiveVersion::V5,
            &mut crate::loss::Diagnostics::new(),
        )
        .expect("actual line class-data");
        let key = IdentityKey::try_new("source".to_owned())
            .expect("source key")
            .then(cadmpeg_ir::identity_key!("."))
            .then(IdentityKey::try_new(format!("root.component-{index}")).expect("child path"));
        let child_id = cadmpeg_ir::ids::CurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
            key,
        );
        let layout = &fidelity.record_layout()[child_id.as_str()];
        assert_eq!(
            layout.offset(),
            u64::try_from(child.class_data_range.start).expect("small start")
        );
        assert_eq!(
            layout.byte_len(),
            u64::try_from(child.class_data_range.len()).expect("small extent")
        );
        assert_eq!(
            layout.reads()["file"],
            [
                u64::try_from(child.class_data_range.start).expect("small start")
                    ..u64::try_from(child.class_data_range.end).expect("small end"),
            ]
        );
        cursor = wrapper.next_offset();
    }
    assert!(start + fixture.parameter_array.end < fields_end);
    assert!(fields_end < bytes.len());
}

#[test]
fn polycurve_source_entry_keeps_the_actual_caller_depth_refusal_latched() {
    let fixture = test_archive::polycurve_source(&[-2.0, 1.0, 6.0], &children());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, source) =
        DecodeContext::from_root_bytes(&fixture.bytes, &arena, &policy).expect("source");
    let key = IdentityKey::try_new("source".to_owned()).expect("source key");
    let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::decode(
        &ctx,
        source,
        &fixture.bytes,
        0..fixture.bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &key,
    )
    .expect_err("caller allowance") else {
        panic!("actual caller resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    let cadmpeg_core::CodecError::ResourceLimit(persisted) = ctx
        .take_record_captures()
        .expect_err("the caller refusal prevents releasing captures")
    else {
        panic!("actual refusal stays latched")
    };
    assert_eq!(persisted, limit);
}

#[test]
fn unbacked_polycurve_child_count_fails_before_parameter_allocation_without_fusing_the_caller() {
    let mut bytes = vec![0x10];
    bytes.extend(i32::MAX.to_le_bytes());
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("source");
    let key = IdentityKey::try_new("source".to_owned()).expect("source key");
    let failure = super::decode(
        &ctx,
        source,
        &bytes,
        0..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &key,
    )
    .expect_err("declared children must fit before any parameter allocation");
    assert!(
        matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength {
        offset: 1, value,
    }) if value == i128::from(i32::MAX))
    );
    ctx.charge_entities(1, "valid peer after malformed polycurve count")
        .expect("malformed count is not a fabricated resource refusal");
    assert!(ctx
        .take_record_captures()
        .expect("failed tree releases no captures")
        .is_empty());
}

#[test]
fn malformed_late_line_child_rolls_back_its_peer_reads_before_the_next_valid_tree() {
    let mut malformed_children = children();
    malformed_children[1].1 =
        test_archive::line_payload([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]);
    let malformed = test_archive::polycurve_source(&[-2.0, 1.0, 6.0], &malformed_children);
    let valid = test_archive::polycurve_source(&[-2.0, 1.0, 6.0], &children());
    let mut bytes = malformed.bytes;
    let peer_start = bytes.len();
    bytes.extend(&valid.bytes);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("actual source containing both trees");
    let failed_key = IdentityKey::try_new("failed".to_owned()).expect("failed source key");
    assert!(matches!(
        super::decode(
            &ctx,
            root,
            &bytes,
            0..peer_start,
            MillimeterScale::IDENTITY,
            ArchiveVersion::V5,
            &failed_key,
        ),
        Err(GeometryError::Malformed(_))
    ));
    ctx.charge_entities(1, "valid peer after malformed native line")
        .expect("malformed coordinates do not fuse the caller allowance");
    let peer_key = IdentityKey::try_new("peer".to_owned()).expect("valid source key");
    let decoded = super::decode(
        &ctx,
        root,
        &bytes,
        peer_start..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
        &peer_key,
    )
    .expect("valid peer with the same actual caller");
    assert_tree(&decoded);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .expect("one extraction after failure and recovery");
    assert_eq!(fidelity.record_layout().len(), 3);
    for (id, layout) in fidelity.record_layout() {
        assert!(!id.contains("failed"));
        assert!(layout.offset() >= u64::try_from(peer_start).expect("small peer start"));
        assert!(!layout.reads().is_empty());
    }
}
