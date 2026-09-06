// SPDX-License-Identifier: Apache-2.0
//! Surface construction branch framing domains.

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum SurfaceFamily {
    Form14 = 0x14,
    Form50 = 0x50,
}

impl From<SurfaceFamily> for u8 {
    fn from(value: SurfaceFamily) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for SurfaceFamily {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x14 => Ok(Self::Form14),
            0x50 => Ok(Self::Form50),
            _ => Err("surface family must be 0x14 or 0x50"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct SurfaceSuffix(Vec<u8>);

impl SurfaceSuffix {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if !(1..=5).contains(&bytes.len()) {
            return Err("surface suffix must contain 1 through 5 bytes");
        }
        Ok(Self(bytes))
    }

    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl<'de> Deserialize<'de> for SurfaceSuffix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{SurfaceFamily, SurfaceSuffix};

    #[test]
    fn surface_family_preserves_exact_numeric_domain() {
        for byte in u8::MIN..=u8::MAX {
            let wire = byte.to_string();
            let decoded = serde_json::from_str::<SurfaceFamily>(&wire);
            if matches!(byte, 0x14 | 0x50) {
                assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), wire);
            } else {
                assert!(decoded.unwrap_err().to_string().contains("family"));
            }
        }
    }

    #[test]
    fn surface_suffix_preserves_opaque_bytes_and_rejects_invalid_lengths() {
        for wire in [
            "[255]",
            "[0,255]",
            "[1,2,3]",
            "[0,1,2,255]",
            "[255,0,1,2,3]",
        ] {
            let decoded: SurfaceSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
            assert!((1..=5).contains(&decoded.into_vec().len()));
        }
        for wire in ["[]", "[0,1,2,3,4,5]"] {
            assert!(serde_json::from_str::<SurfaceSuffix>(wire)
                .unwrap_err()
                .to_string()
                .contains("suffix"));
        }
    }
}
