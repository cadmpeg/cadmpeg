// SPDX-License-Identifier: Apache-2.0
//! Encoding detection from the first bytes of a layer (spec §1, §3.1, §9).

use cadmpeg_ir::codec::Confidence;

use crate::layout::usdc_bootstrap as bootstrap;

/// The text header cookies of §3.1.
const USDA_COOKIE: &[u8] = b"#usda ";
const SDF_COOKIE: &[u8] = b"#sdf ";
/// Longest version token read from a text header.
const MAX_VERSION_TOKEN: usize = 32;

/// The header of a layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Header {
    /// A text layer: its cookie and the version token after it.
    Text { cookie: Cookie, version: String },
    /// A binary layer and its major, minor, and patch version.
    Binary { version: [u8; 3] },
}

/// The cookie of a text header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cookie {
    Usda,
    Sdf,
}

impl Cookie {
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Usda => "#usda",
            Self::Sdf => "#sdf",
        }
    }
}

/// The header of the layer that starts with `prefix`, or `None` when the
/// bytes are not an `OpenUSD` layer.
pub(crate) fn header(prefix: &[u8]) -> Option<Header> {
    for (cookie, bytes) in [(Cookie::Usda, USDA_COOKIE), (Cookie::Sdf, SDF_COOKIE)] {
        if let Some(rest) = prefix.strip_prefix(bytes) {
            let token: Vec<u8> = rest
                .iter()
                .copied()
                .take_while(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
                .take(MAX_VERSION_TOKEN)
                .collect();
            let version = String::from_utf8(token).ok()?;
            return (!version.is_empty()).then_some(Header::Text { cookie, version });
        }
    }
    if prefix.get(bootstrap::IDENTIFIER..bootstrap::MAJOR)? != bootstrap::IDENTIFIER_VALUE {
        return None;
    }
    let version = [
        *prefix.get(bootstrap::MAJOR)?,
        *prefix.get(bootstrap::MINOR)?,
        *prefix.get(bootstrap::PATCH)?,
    ];
    Some(Header::Binary { version })
}

pub(crate) fn confidence(prefix: &[u8]) -> Confidence {
    match header(prefix) {
        Some(_) => Confidence::High,
        None => Confidence::No,
    }
}

#[cfg(test)]
mod tests;
