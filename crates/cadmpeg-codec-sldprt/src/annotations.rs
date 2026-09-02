// SPDX-License-Identifier: Apache-2.0
//! SLDPRT helpers for appending sparse IR annotations.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use cadmpeg_ir::annotations::{Annotations, ExactnessNote};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

pub(crate) fn note(
    annotations: &mut Annotations,
    id: impl Into<String>,
    stream: impl Into<String>,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let id = id.into();
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let stream = builder.stream(stream);
    builder.note(&id, stream, offset).tag(tag);
    *annotations = builder.build();
    if exactness == Exactness::ByteExact {
        annotations.exactness.remove(&id);
    } else {
        annotations.exactness.insert(
            id,
            ExactnessNote {
                entity: exactness,
                fields: BTreeMap::default(),
            },
        );
    }
}
