// SPDX-License-Identifier: Apache-2.0
//! Closed THRU_CURVE branch suffixes and group terminators.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u8; 2]", into = "[u8; 2]")]
pub(crate) enum ThruCurveBranchSuffix {
    Code48,
    Code58,
}

impl TryFrom<[u8; 2]> for ThruCurveBranchSuffix {
    type Error = &'static str;

    fn try_from(bytes: [u8; 2]) -> Result<Self, Self::Error> {
        match bytes {
            [0x81, 0x48] => Ok(Self::Code48),
            [0x81, 0x58] => Ok(Self::Code58),
            _ => Err("suffix must be [129, 72] or [129, 88]"),
        }
    }
}

impl From<ThruCurveBranchSuffix> for [u8; 2] {
    fn from(value: ThruCurveBranchSuffix) -> Self {
        match value {
            ThruCurveBranchSuffix::Code48 => [0x81, 0x48],
            ThruCurveBranchSuffix::Code58 => [0x81, 0x58],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub(crate) enum ThruCurveGroupTerminator {
    Separated,
    Adjacent,
}

impl ThruCurveGroupTerminator {
    pub(crate) const ALL: [Self; 2] = [Self::Separated, Self::Adjacent];

    pub(crate) const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Separated => &[0, 0, 0, 0, 0, 0, 0xff, 0, 0xff, 1],
            Self::Adjacent => &[0, 0, 0, 0, 0, 0, 0xff, 0xff, 1],
        }
    }
}

impl TryFrom<Vec<u8>> for ThruCurveGroupTerminator {
    type Error = &'static str;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|value| value.bytes() == bytes)
            .ok_or("terminator must be a THRU_CURVE group terminator")
    }
}

impl From<ThruCurveGroupTerminator> for Vec<u8> {
    fn from(value: ThruCurveGroupTerminator) -> Self {
        value.bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{ThruCurveBranchSuffix, ThruCurveGroupTerminator};

    #[test]
    fn branch_suffix_wire_is_closed() {
        for wire in ["[129,72]", "[129,88]"] {
            let value: ThruCurveBranchSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        }
        for wire in ["[128,72]", "[129,73]", "[129]", "[129,72,0]"] {
            assert!(serde_json::from_str::<ThruCurveBranchSuffix>(wire).is_err());
        }
    }

    #[test]
    fn group_terminator_wire_is_closed() {
        for wire in ["[0,0,0,0,0,0,255,0,255,1]", "[0,0,0,0,0,0,255,255,1]"] {
            let value: ThruCurveGroupTerminator = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        }
        for wire in [
            "[]",
            "[0,0,0,0,0,0,255,255,0]",
            "[0,0,0,0,0,0,255,0,0,255,1]",
        ] {
            assert!(serde_json::from_str::<ThruCurveGroupTerminator>(wire).is_err());
        }
    }
}
