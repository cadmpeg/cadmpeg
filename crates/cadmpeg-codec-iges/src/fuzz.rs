// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

/// Exercise Fixed ASCII physical-line and card framing.
pub fn cards(data: &[u8]) {
    let _ = crate::card::scan_with_context(data, None);
}

/// Exercise Global section delimiter and Hollerith parsing.
pub fn global(data: &[u8]) {
    let Ok(scan) = crate::card::scan_with_context(data, None) else {
        return;
    };
    let _ = crate::global::parse(&scan);
}

/// Exercise Directory Entry pair parsing.
pub fn directory(data: &[u8]) {
    let Ok(scan) = crate::card::scan_with_context(data, None) else {
        return;
    };
    let _ = crate::directory::parse(&scan);
}

/// Exercise Parameter Data assembly from directory-owned cards.
pub fn parameters(data: &[u8]) {
    let Ok(scan) = crate::card::scan_with_context(data, None) else {
        return;
    };
    let Ok(global) = crate::global::parse(&scan) else {
        return;
    };
    let Ok(directory) = crate::directory::parse(&scan) else {
        return;
    };
    let _ = crate::parameter::assemble_with_context(&scan, &directory, &global, None);
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
}
