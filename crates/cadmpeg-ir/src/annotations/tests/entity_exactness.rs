// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness};

const ID: &str = "test:model:point#exactness";

#[test]
fn entity_exactness_moves_an_owned_identity() {
    let identity = crate::ids::Identity::new(ID).unwrap();
    let pointer = identity.as_str().as_ptr();
    let mut builder = AnnotationBuilder::new();
    builder.exactness(identity, Exactness::Derived);
    let (identity, note) = builder.annotations().exactness().first_key_value().unwrap();
    assert_eq!(identity.as_str().as_ptr(), pointer);
    assert_eq!(identity, ID);
    assert_eq!(note.entity(), Exactness::Derived);
}
