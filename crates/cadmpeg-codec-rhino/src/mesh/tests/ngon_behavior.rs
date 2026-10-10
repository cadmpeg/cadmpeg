// SPDX-License-Identifier: Apache-2.0
use super::super::{read_ngons, MeshExpand};
use super::{chunk, with_expand};
use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::loss::Diagnostics;

#[test]
fn current_ngon_count_and_index_bounds_preserve_decode_behavior() {
    for (words, expected) in [
        (&[1_u32, 0, 1, 3, 1, 0, 1, 3, 0][..], 1),
        (&[1_u32, 0, 1, 3, 1, 0, 1, 2, 1][..], 1),
        (&[1_u32, 0, 1, 0][..], 1),
        (&[1_u32, 0, 3, 0, 3, 1, 0, 1, 2, 0, 0][..], 3),
    ] {
        let bytes = chunk(&words.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>());
        with_expand(&bytes, |expand: MeshExpand<'_>| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            assert_eq!(read_ngons(expand.ctx(), &mut reader, ArchiveVersion::V8, 3, 1,
                &mut Diagnostics::new()).unwrap(), expected);
            assert_eq!(reader.remaining(), 0);
            assert!(expand.ctx().resource_refusal().is_none());
        });
    }
}
