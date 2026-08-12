// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn with_source(data: &[u8], run: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>)) {
    let arena = DecodeArena::new();
    let Ok((ctx, source)) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())
    else {
        return;
    };
    run(&ctx, source);
}

/// Exercise ZIP entry classification and ASM header scanning.
pub fn container(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::container::scan(ctx, source);
    });
}

/// Exercise nested Protein ZIP member lookup used during appearance decode.
pub fn nested_archive(data: &[u8]) {
    with_source(data, |ctx, source| {
        let _ = crate::materials::nested_entry(ctx, source, "AssetData/InstanceProperties.bin");
        let _ = crate::materials::nested_entry(
            ctx,
            source,
            "AssetData/DefinitionIteratorProperties.bin",
        );
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::container(&[]);
        super::nested_archive(&[]);
    }
}
