// SPDX-License-Identifier: Apache-2.0
//! Closed `THRU_CURVE` branch suffixes and group terminators.

use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NativeBytes<[u8; 2]>", into = "NativeBytes<[u8; 2]>")]
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

impl TryFrom<NativeBytes<[u8; 2]>> for ThruCurveBranchSuffix {
    type Error = &'static str;
    fn try_from(bytes: NativeBytes<[u8; 2]>) -> Result<Self, Self::Error> {
        Self::try_from(bytes.into_inner())
    }
}
impl From<ThruCurveBranchSuffix> for NativeBytes<[u8; 2]> {
    fn from(value: ThruCurveBranchSuffix) -> Self {
        <[u8; 2]>::from(value).into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "NativeBytes<Vec<u8>>")]
pub(crate) enum ThruCurveGroupTerminator {
    Separated,
    Adjacent,
}

impl Serialize for ThruCurveGroupTerminator {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        NativeBytes::from(self.bytes()).serialize(serializer)
    }
}

#[cfg(test)]
std::thread_local! {
    static TERMINATOR_INTO_VEC_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl ThruCurveGroupTerminator {
    pub(super) const ALL: [Self; 2] = [Self::Separated, Self::Adjacent];

    pub(super) const fn bytes(self) -> &'static [u8] {
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

impl TryFrom<NativeBytes> for ThruCurveGroupTerminator {
    type Error = &'static str;
    fn try_from(bytes: NativeBytes) -> Result<Self, Self::Error> {
        Self::try_from(bytes.into_inner())
    }
}

#[cfg(test)]
impl From<ThruCurveGroupTerminator> for Vec<u8> {
    fn from(value: ThruCurveGroupTerminator) -> Self {
        TERMINATOR_INTO_VEC_COUNT.with(|count| count.set(count.get() + 1));
        value.bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeBytes, ThruCurveBranchSuffix, ThruCurveGroupTerminator, TERMINATOR_INTO_VEC_COUNT,
    };

    #[test]
    fn branch_suffix_wire_is_closed() {
        for wire in ["\"8148\"", "\"8158\""] {
            let value: ThruCurveBranchSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        }
        for wire in ["\"8048\"", "\"8149\"", "\"81\"", "\"814800\""] {
            assert!(serde_json::from_str::<ThruCurveBranchSuffix>(wire).is_err());
        }
    }

    #[test]
    fn group_terminator_wire_is_closed() {
        for wire in ["\"000000000000ff00ff01\"", "\"000000000000ffff01\""] {
            let value: ThruCurveGroupTerminator = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
            assert_eq!(
                serde_json::to_vec(&value).unwrap(),
                serde_json::to_vec(&NativeBytes::from(Vec::<u8>::from(value))).unwrap()
            );
        }
        for wire in [
            "\"\"",
            "\"000000000000ffff00\"",
            "\"000000000000ff0000ff01\"",
        ] {
            assert!(serde_json::from_str::<ThruCurveGroupTerminator>(wire).is_err());
        }
    }

    #[test]
    fn group_terminator_retained_limit_refuses_before_vec_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            terminator: &'a ThruCurveGroupTerminator,
        }

        for value in ThruCurveGroupTerminator::ALL {
            let record = Record {
                id: "nx:thru-curve:terminator#1",
                terminator: &value,
            };
            TERMINATOR_INTO_VEC_COUNT.with(|count| count.set(0));
            cadmpeg_test_support::native_serialization::assert_native_limit(
                &record,
                serde_json::json!({"id": record.id, "terminator": NativeBytes::from(value.bytes())}),
            );
            TERMINATOR_INTO_VEC_COUNT.with(|count| assert_eq!(count.get(), 0));
        }
    }
}
