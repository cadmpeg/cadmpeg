use crate::test_support::{lp_ascii, push_reference_u64};
use cadmpeg_ir::features::{CapturedPositionTransform, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::transform::Transform;
use std::io::{Cursor, Read, Write};

#[test]
fn localized_position_decodes_dispatches_projects_and_binds_after_products() {
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
    let base = crate::test_support::assembly_test::f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("part.f3d", role)],
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
    let decoded = super::decode(zip.finish().unwrap().into_inner());
    let native = crate::test_support::native_test::f3d_native(decoded.ir());
    let scope = native
        .design_parameter_scopes
        .iter()
        .find(|scope| scope.record_index == 500)
        .unwrap();
    assert_eq!(scope.source_kind_name(), "Posición");
    assert_eq!(scope.kind_name(), "Position");
    assert_eq!(
        native.design_feature_timelines[0].frame().items()[0].value,
        500
    );
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        .unwrap();
    assert_eq!(feature.ordinal, 0);
    let FeatureDefinition::Operation(FeatureOperation::CapturedComponentPositions { positions }) =
        feature.evaluation.definition()
    else {
        panic!("captured positions")
    };
    assert_eq!(positions.len(), 2);
    assert_eq!(
        positions[0].transform,
        CapturedPositionTransform::Affine {
            transform: Transform::affine([
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 1.0, 0.0, -30.0],
                [0.0, 0.0, 1.0, 40.0]
            ])
            .unwrap(),
        }
    );
    let occurrence = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| Some(&occurrence.id) == positions[0].occurrence.as_ref())
        .unwrap();
    assert_eq!(occurrence.transform, Transform::identity());
    assert!(positions[0]
        .selector
        .as_str()
        .ends_with(":design-record#700"));
    assert!(positions[1]
        .selector
        .as_str()
        .ends_with(":design-record#703"));
    assert!(positions[1].occurrence.is_none());
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::PositionOccurrenceUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("row 1"));
    super::assert_valid(&decoded);
}
