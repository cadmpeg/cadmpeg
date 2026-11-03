// SPDX-License-Identifier: Apache-2.0
//! Deltas stream wrappers around the shared Parasolid BODY witnesses.

#![allow(clippy::unwrap_used)]

pub(crate) fn stream(bytes: Vec<u8>) -> crate::parasolid::Stream {
    crate::parasolid::Stream {
        file_offset: 0,
        consumed: u64::try_from(bytes.len()).unwrap(),
        compressed: true,
        inflated: bytes,
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_3501171_35102_13006").unwrap(),
            ),
        },
    }
}
