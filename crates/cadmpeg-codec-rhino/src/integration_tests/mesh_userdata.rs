// SPDX-License-Identifier: Apache-2.0
//! Mesh-owned class-userdata admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;

fn mesh_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, 0x20);
    let mut uuid_body = support::MESH_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&support::MESH_CLASS).to_le_bytes());
    let class_uuid = support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::mesh_payload(3, 5, false, true),
    );
    let class_end = support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata.to_vec(), class_end].concat(),
    );
    let object_end = support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

#[test]
fn mesh_correspondence_future_payload_retains_parent_mesh_record() {
    let archive = ArchiveVersion::V5;
    let application = crate::wire::Uuid::from_canonical([
        0x94, 0x21, 0xee, 0x97, 0xe8, 0x95, 0x47, 0xbc, 0x99, 0xeb, 0x5f, 0xd1, 0xba, 0x35, 0xb3,
        0x67,
    ])
    .to_wire();
    let future_payload = [2_i32.to_le_bytes().as_slice(), [0xde, 0xad].as_slice()].concat();
    for (class, label) in [
        (
            crate::mesh::TT_MAPPING_MESH_INFO_USERDATA,
            "CTtMappingMeshInfoUserData",
        ),
        (
            crate::mesh::TT_RENDER_MESH_INFO_USERDATA,
            "CTtRenderMeshInfoUserData",
        ),
    ] {
        let userdata = support::test_dump::class_userdata_v2_with_direct_payload(
            archive,
            class.to_wire(),
            application,
            50,
            0,
            &future_payload,
        );
        let mesh_record = mesh_record(archive, &userdata);
        let following_point = support::test_dump::object_record_with_payload(
            archive,
            1,
            support::test_dump::POINT_CLASS,
            &support::point_payload([4.0, 5.0, 6.0]),
        );
        let result = decode(support::archive_writer(
            "50",
            202_608_010,
            &[mesh_record.clone(), following_point],
        ));

        assert_eq!(result.ir().model.tessellations.len(), 1);
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(result.report().losses.iter().any(|loss| {
            loss.message.contains(label) && loss.message.contains("could not be transferred")
        }));
        let retained = result
            .source_fidelity()
            .retained_records
            .iter()
            .find(|record| record.id == "rhino:object:record#000000")
            .expect("mesh correspondence object record is retained");
        assert_eq!(retained.data.as_deref(), Some(mesh_record.as_slice()));
        assert_valid(&result);
    }
}
