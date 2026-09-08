// SPDX-License-Identifier: Apache-2.0
//! The optional one-, two-, or three-byte value before an aligned control array.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControlLeadingValue(LeadingBytes);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadingBytes {
    One(u8),
    Two([u8; 2]),
    Three([u8; 3]),
}

impl ControlLeadingValue {
    pub(crate) fn read(width: usize, mut bytes: impl Iterator<Item = u8>) -> Option<Self> {
        Some(Self(match width {
            1 => LeadingBytes::One(bytes.next()?),
            2 => LeadingBytes::Two([bytes.next()?, bytes.next()?]),
            3 => LeadingBytes::Three([bytes.next()?, bytes.next()?, bytes.next()?]),
            _ => return None,
        }))
    }

    pub(crate) fn from_wire(width: u8, value: u32) -> Result<Self, &'static str> {
        let result = Self::read(usize::from(width), value.to_le_bytes().into_iter())
            .ok_or("leading_value_width: must be 1, 2, or 3")?;
        if result.value() != value {
            return Err("leading_value: exceeds leading_value_width");
        }
        Ok(result)
    }

    pub(crate) fn width(self) -> u8 {
        match self.0 {
            LeadingBytes::One(_) => 1,
            LeadingBytes::Two(_) => 2,
            LeadingBytes::Three(_) => 3,
        }
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            LeadingBytes::One(value) => u32::from(value),
            LeadingBytes::Two([low, high]) => u32::from(low) | (u32::from(high) << 8),
            LeadingBytes::Three([low, middle, high]) => {
                u32::from(low) | (u32::from(middle) << 8) | (u32::from(high) << 16)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ControlLeadingValue;

    #[test]
    fn leading_values_retain_width_even_when_zero() {
        for (width, maximum) in [(1, 0xff), (2, 0xffff), (3, 0xff_ffff)] {
            for value in [0, maximum] {
                let leading = ControlLeadingValue::from_wire(width, value).unwrap();
                assert_eq!((leading.width(), leading.value()), (width, value));
            }
            assert!(ControlLeadingValue::from_wire(width, maximum + 1).is_err());
        }
        for width in [0, 4, 255] {
            assert!(ControlLeadingValue::from_wire(width, 0).is_err());
        }
        assert!(ControlLeadingValue::read(3, [1, 2].into_iter()).is_none());
    }
}
