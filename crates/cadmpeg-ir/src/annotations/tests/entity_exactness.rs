// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness};

const ID: &str = "test:model:point#exactness";

#[test]
fn entity_exactness_moves_an_owned_identity() {
    let identity = ID.to_owned();
    let pointer = identity.as_ptr();
    let mut builder = AnnotationBuilder::new();
    builder.exactness_owned(identity, Exactness::Derived);
    let (identity, note) = builder.annotations().exactness().first_key_value().unwrap();
    assert_eq!(identity.as_ptr(), pointer);
    assert_eq!(identity, ID);
    assert_eq!(note.entity(), Exactness::Derived);
}
