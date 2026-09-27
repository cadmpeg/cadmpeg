// SPDX-License-Identifier: Apache-2.0
//! Native arena serialization limit checks for codec-owned record tests.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::native::{arena_from, NativeConvertError};
use serde::Serialize;
use std::cell::Cell;

struct Counted<'a, T> {
    record: &'a T,
    visits: &'a Cell<usize>,
}

impl<T: Serialize> Serialize for Counted<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.visits.set(self.visits.get() + 1);
        self.record.serialize(serializer)
    }
}

/// Check a record's one-pass admission, retained-byte refusal, and stored wire.
pub fn assert_native_limit<T: Serialize>(record: &T, expected: impl Into<serde_json::Value>) {
    let expected = expected.into();
    let visits = Cell::new(0);
    let counted = Counted {
        record,
        visits: &visits,
    };
    let needed = serde_json::to_vec(&counted).expect("valid fixture").len();
    visits.set(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).expect("fixture length") - 1;
    let (limited, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let error = arena_from(&limited, [Ok::<_, NativeConvertError>(&counted)])
        .expect_err("retained-byte refusal");
    assert_eq!(visits.get(), 1);
    assert!(matches!(cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"));

    visits.set(0);
    let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let stored =
        arena_from(&service, [Ok::<_, NativeConvertError>(&counted)]).expect("service admission");
    assert_eq!(visits.get(), 1);
    assert_eq!(
        serde_json::to_value(&stored[0]).expect("stored record"),
        expected
    );
}
