// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

/// Exercise IGES physical-card scanning.
pub fn cards(data: &[u8]) {
    let _probe = crate::card::scan_with_context(data, None);
}

/// Exercise IGES global-section parsing.
pub fn global(data: &[u8]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    else {
        return;
    };
    let Ok(scan) = crate::card::scan_with_context(data, Some(&ctx)) else {
        return;
    };
    let _probe = crate::global::parse(&scan, &ctx);
}

/// Exercise IGES directory-section parsing.
pub fn directory(data: &[u8]) {
    let Ok(scan) = crate::card::scan_with_context(data, None) else {
        return;
    };
    let (_typed, _quarantined) = crate::directory::parse(&scan, crate::global::GlobalTable::Legacy);
}

/// Exercise IGES parameter-section assembly.
pub fn parameters(data: &[u8]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    else {
        return;
    };
    let Ok(scan) = crate::card::scan_with_context(data, Some(&ctx)) else {
        return;
    };
    let Ok((global, _)) = crate::global::parse(&scan, &ctx) else {
        return;
    };
    let (directory, quarantined) = crate::directory::parse(&scan, global.global_table());
    let _probe =
        crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, None);
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::cards(&[]);
        super::global(&[]);
        super::directory(&[]);
        super::parameters(&[]);
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::test_curves_and_surfaces::point_file();
        super::cards(&data);
        super::global(&data);
        super::directory(&data);
        super::parameters(&data);
    }
}
