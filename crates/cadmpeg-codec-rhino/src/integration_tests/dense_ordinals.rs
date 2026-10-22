// SPDX-License-Identifier: Apache-2.0
//! Feature ordinals follow admitted feature order, not source object positions.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    crc_table, minimal_document, object_record_with_payload, point_payload, table, units_record,
    POINT_CLASS,
};

#[test]
fn mixed_object_history_and_morph_features_keep_dense_ordinals_and_source_links() {
    let archive = ArchiveVersion::V5;
    let point =
        || object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let records = [
        point(),
        object_record_with_payload(archive, 0x1_0000, crate::hatch::CLASS.to_wire(), &[]),
        object_record_with_payload(
            archive,
            0x1_0000,
            crate::hatch::CLASS.to_wire(),
            &crate::hatch::tests::version_two_hatch_payload(),
        ),
        point(),
        object_record_with_payload(
            archive,
            4,
            crate::polyedge::CURVE_CLASS.to_wire(),
            &crate::polyedge::tests::polyedge_payload(),
        ),
        object_record_with_payload(
            archive,
            0x2_0000,
            crate::morph::CLASS.to_wire(),
            &crate::morph::tests::cage_morph_payload(),
        ),
        point(),
    ];
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[units_record(archive, 2)]),
            table(archive, 0x1000_0013, &records),
            crc_table(
                archive,
                0x1000_0026,
                &[crate::history::tests::source_band_history_record(
                    archive, 1,
                )],
            ),
        ],
    );
    let scan = crate::container::scan_owned(bytes.clone()).expect("original bounded source");
    assert_eq!(scan.objects.len(), 7);
    assert_eq!(scan.history.len(), 1);
    let result = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("mixed source records decode");
    let mut features = result.ir().model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    assert_eq!(features.len(), 4, "{:?}", result.report());
    for (index, (tag, native_ref)) in [
        ("RhinoHatch", "rhino:object:record#000002"),
        ("RhinoPolyEdgeReference", "rhino:object:record#000004"),
        ("RhinoMorphControl", "rhino:object:record#000005"),
        (
            "HistoryRecord",
            "rhino:history:record#00000000-0000-0000-0000-000000000001",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            features[index].ordinal,
            u64::try_from(index).expect("small index")
        );
        assert_eq!(features[index].source_tag.as_deref(), Some(tag));
        assert_eq!(features[index].native_ref.as_deref(), Some(native_ref));
    }
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(!features
        .iter()
        .any(|feature| feature.native_ref.as_deref() == Some("rhino:object:record#000001")));
}
