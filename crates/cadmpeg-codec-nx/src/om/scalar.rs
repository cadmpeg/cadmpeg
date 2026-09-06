// SPDX-License-Identifier: Apache-2.0
//! Checked shifted binary64 atoms and their decoded values.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShiftedBinary64([u8; 8]);

impl ShiftedBinary64 {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        Self::try_from(<[u8; 8]>::try_from(bytes).ok()?).ok()
    }

    pub(crate) fn raw(self) -> [u8; 8] { self.0 }

    pub(crate) fn value(self) -> f64 {
        let mut bytes = self.0;
        bytes[0] += 0x10;
        f64::from_be_bytes(bytes)
    }

    pub(crate) fn from_wire(value: f64, raw: [u8; 8]) -> Result<Self, &'static str> {
        let atom = Self::try_from(raw)?;
        if value.to_bits() != atom.value().to_bits() {
            return Err("scalars must match raw_scalars");
        }
        Ok(atom)
    }
}

impl TryFrom<[u8; 8]> for ShiftedBinary64 {
    type Error = &'static str;

    fn try_from(bytes: [u8; 8]) -> Result<Self, Self::Error> {
        if !is_shifted_ieee_f64_marker(bytes[0]) {
            return Err("raw_scalars must contain shifted binary64 markers");
        }
        // These marker intervals map only to finite binary64 exponent ranges.
        Ok(Self(bytes))
    }
}

pub(super) fn shifted_ieee_f64(bytes: &[u8]) -> Option<f64> {
    ShiftedBinary64::read(bytes).map(ShiftedBinary64::value)
}

pub(super) fn is_shifted_ieee_f64_marker(marker: u8) -> bool {
    matches!(marker, 0x20..=0x3f | 0xa0..=0xbf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_binary64_retains_bytes_and_derives_the_value() {
        let mut raw = 1.0_f64.to_be_bytes();
        raw[0] -= 0x10;
        let atom = ShiftedBinary64::try_from(raw).unwrap();
        assert_eq!(atom.value(), 1.0);
        assert_eq!(atom.raw(), raw);
        assert_eq!(ShiftedBinary64::from_wire(1.0, raw), Ok(atom));
        assert!(ShiftedBinary64::from_wire(2.0, raw).is_err());
        for marker in [0x20, 0x3f, 0xa0, 0xbf] {
            raw[0] = marker;
            assert!(ShiftedBinary64::try_from(raw).unwrap().value().is_finite());
        }
        for marker in [0x00, 0x1f, 0x40, 0x9f, 0xc0, 0xff] {
            raw[0] = marker;
            assert!(ShiftedBinary64::try_from(raw).is_err());
        }
        assert!(ShiftedBinary64::read(&[0; 7]).is_none());
    }
}
