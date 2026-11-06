// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn malformed_protein_schema_element_keeps_sibling_records() {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut protein = zip::ZipWriter::new(Cursor::new(Vec::new()));
    protein
        .start_file("Schemas/KnownSchema.xml", stored)
        .unwrap();
    protein
        .write_all(
            br#"<Schema><UID val="KnownSchema"/><Float id="broken" id="duplicate"/></Schema>"#,
        )
        .unwrap();
    protein
        .start_file("AssetData/InstanceProperties.bin", stored)
        .unwrap();
    protein.write_all(&three_record_instance()).unwrap();
    let protein = protein.finish().unwrap().into_inner();

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut archive, stored);
    archive
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    archive.write_all(&protein).unwrap();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(archive.finish().unwrap().into_inner()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let guids: Vec<_> = decoded
        .ir()
        .model
        .appearances
        .iter()
        .filter_map(|appearance| appearance.asset_guid.as_deref())
        .collect();
    assert_eq!(guids, ["first-guid", "third-guid"]);
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.local_code() == "record.unreadable" && loss.message.contains("<Float>")
        })
        .collect();
    assert_eq!(losses.len(), 1);
}
