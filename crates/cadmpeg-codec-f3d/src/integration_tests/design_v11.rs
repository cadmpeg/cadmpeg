// SPDX-License-Identifier: Apache-2.0
//! A decoded Fusion design states its timeline in the v11 design layer.

use crate::test_support::{lp_ascii, push_reference_u64};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::report::check::Check;
use std::io::{Cursor, Read, Write};

/// A design whose one timeline item is a captured `Position`.
fn position_design() -> Vec<u8> {
    position_design_on(crate::test_support::assembly_test::f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("part.f3d", crate::test_support::assembly_test::XREF_ROLE)],
    ))
}

/// The `position_design` streams added to the archive `base`.
fn position_design_on(base: Vec<u8>) -> Vec<u8> {
    const PLACEMENT: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";
    const POSITION: &str = "8EE00B00-76BB-49AB-8C25-E837FEC5BDA5";
    const LOCATOR: &str = "2E3AC990-483D-4A55-8662-C064B976A957";
    const PATH: &str = "96CBEA21-17E0-4BCB-BA77-04EA87B47934";
    const TIMELINE: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
    const TIMELINE_BASE: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
    const OTHER: &str = "00000000-0000-4000-8000-000000000002";
    let matrix = [
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, -3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (mut bulk, _, mut types) = crate::test_support::position::fixture(&[Some(matrix), None]);
    let locator_at = bulk.len();
    crate::test_support::position::append_locator_path(
        &mut bulk,
        &mut types,
        500,
        &["66666666-7777-8888-9999-aaaaaaaaaaaa"],
        &[],
    );
    let placement_at = bulk.len();
    let role = crate::test_support::assembly_test::XREF_ROLE;
    let mut placement =
        crate::test_support::legacy_attributed_external_placement(role, false, None);
    placement[4..7].copy_from_slice(b"256");
    bulk.extend(placement);
    let timeline_at = bulk.len();
    lp_ascii(&mut bulk, "263");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut bulk, 500);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut bulk, 500);
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (PLACEMENT, "", 2, "Component", &[10]),
            (POSITION, "", 6, "Fusion", &[500]),
            (OTHER, "", 0, "Fusion", &[]),
            (LOCATOR, "", 5, "Fusion", &[700]),
            (PATH, "", 1, "Fusion", &[701]),
            (OTHER, "", 0, "Fusion", &[702]),
            (OTHER, "", 0, "Fusion", &[703]),
            (TIMELINE, TIMELINE_BASE, 2, "Fusion", &[35]),
        ],
        &[
            (500, 0),
            (700, u64::try_from(locator_at).unwrap()),
            (10, u64::try_from(placement_at).unwrap()),
            (35, u64::try_from(timeline_at).unwrap()),
        ],
    );
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    for (name, bytes) in [("BulkStream.dat", bulk), ("MetaStream.dat", meta)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn geometry_depth_keeps_the_model_without_projecting_the_design_timeline() {
    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::DecodeOptions;

    let mut zip = zip::ZipWriter::new_append(Cursor::new(position_design())).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&crate::test_support::smbh_geometry_test::synthetic_geometry_smbh())
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let full = super::decode(bytes.clone());
    let geometry = crate::test_support::archive_session::decode(
        &mut Cursor::new(bytes),
        &DecodeOptions {
            depth: DecodeDepth::Geometry,
            ..DecodeOptions::default()
        },
    )
    .expect("geometry decode with a saved design timeline");

    assert_eq!(geometry.report().depth(), DecodeDepth::Geometry);
    assert_eq!(geometry.ir().scope().depth(), DecodeDepth::Geometry);
    assert!(geometry.ir().design.containers.is_empty());
    assert_eq!(full.ir().design.containers.len(), 1);
    assert!(!geometry.ir().geometry.bodies.is_empty());
    assert_eq!(geometry.ir().geometry, full.ir().geometry);
    assert_eq!(geometry.ir().model.occurrences, full.ir().model.occurrences);
}

#[test]
fn a_timeline_is_one_ordered_container_of_the_root_scope() {
    let decoded = super::decode(position_design());
    let ir = decoded.ir();
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope: {:#?}", ir.design.scopes)
    };
    assert_eq!(scope.definition, crate::tree::file_root_id());
    let [container] = ir.design.containers.as_slice() else {
        panic!("one container")
    };
    assert_eq!(scope.roots, [container.id().clone()]);
    let Container::Ordered {
        kind,
        members,
        rollback,
        ..
    } = container
    else {
        panic!("ordered container")
    };
    assert_eq!(*kind, OrderedKind::Timeline);
    assert_eq!(*rollback, Some(Rollback::End));
    let rollback_losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::HistoryRollbackUnread.kind())
        .collect::<Vec<_>>();
    let [loss] = rollback_losses.as_slice() else {
        panic!("one rollback loss: {rollback_losses:#?}")
    };
    assert!(
        loss.message.contains(container.id().as_str()),
        "{}",
        loss.message
    );
    let position = ir
        .model
        .features
        .iter()
        .find(|feature| feature.ordinal == 0)
        .unwrap();
    assert_eq!(members, &[Member::Feature(position.id.clone())]);
    let [feature] = ir.design.features.as_slice() else {
        panic!("one feature")
    };
    assert_eq!(feature.id, position.id);
    assert_eq!(feature.native_ref, position.native_ref);
    assert!(matches!(
        &feature.definition.operation,
        FeatureOperation::Native { kind, .. }
            if kind.as_str() == "f3d:captured_component_positions"
    ));
    let report = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
    let design = report
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Design)
        .collect::<Vec<_>>();
    assert!(design.is_empty(), "{design:#?}");
}

#[test]
fn a_geometry_scope_decode_states_no_design_scope() {
    let bytes = position_design_on(super::f3d_with_smbh(&super::synthetic_geometry_smbh()));
    let full = super::decode(bytes.clone());
    assert!(
        !full.ir().design.scopes.is_empty(),
        "the fixture states a design scope at full depth"
    );
    let geometry = super::EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..cadmpeg_ir::codec::DecodeOptions::default()
            },
        )
        .expect("geometry-scope decode succeeds"),
    );
    assert_eq!(
        geometry.report().depth(),
        cadmpeg_core::decode::DecodeDepth::Geometry
    );
    let ir = geometry.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(ir.design.features.is_empty());
    assert!(ir.design.parameters.is_empty());
    assert!(ir.design.configurations.is_empty());
}
