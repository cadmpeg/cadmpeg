// SPDX-License-Identifier: Apache-2.0
//! SLDPRT helpers for appending sparse IR annotations.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use cadmpeg_ir::annotations::{Annotations, ExactnessNote, Provenance};
use cadmpeg_ir::provenance::Exactness;

pub(crate) fn note(
    annotations: &mut Annotations,
    id: impl Into<String>,
    stream: impl Into<String>,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = stream.into();
    let stream = annotations
        .streams
        .iter()
        .position(|existing| existing == &stream)
        .unwrap_or_else(|| {
            annotations.streams.push(stream);
            annotations.streams.len() - 1
        }) as u32;
    let id = id.into();
    annotations.provenance.insert(
        id.clone(),
        Provenance {
            stream,
            offset,
            tag: Some(tag.to_string()),
        },
    );
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
