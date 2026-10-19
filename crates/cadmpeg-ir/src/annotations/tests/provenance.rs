// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, StreamHandle};

const ID: &str = "test:model:point#provenance";

#[test]
fn provenance_moves_an_owned_identity() {
    let stream = StreamHandle::new(crate::stream_name!("source"));
    let identity = ID.to_owned();
    let pointer = identity.as_ptr();
    let mut builder = AnnotationBuilder::new();
    builder.note_owned(identity, &stream, 7, Some("point"));
    let (identity, provenance) = builder.annotations().provenance.first_key_value().unwrap();
    assert_eq!(identity.as_ptr(), pointer);
    assert_eq!(identity, ID);
    assert_eq!(provenance.stream(), "source");
    assert_eq!(provenance.offset, 7);
    assert_eq!(provenance.tag.as_deref(), Some("point"));
}
