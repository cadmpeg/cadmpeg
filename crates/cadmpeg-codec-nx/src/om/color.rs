// SPDX-License-Identifier: Apache-2.0
//! Palette indices and exact normalized color atoms.

use super::scalar::ShiftedScalar;

pub(crate) const PALETTE_SIZE: usize = 216;
pub(crate) const BACKGROUND_NAME: &str = "Background";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PaletteIndex(u8);

impl PaletteIndex {
    pub(crate) fn new(value: u16) -> Option<Self> {
        (1..=216).contains(&value).then_some(Self(value as u8))
    }

    pub(crate) fn all() -> [Self; PALETTE_SIZE] {
        std::array::from_fn(|ordinal| Self(ordinal as u8 + 1))
    }

    pub(crate) fn value(self) -> u16 {
        u16::from(self.0)
    }

    pub(crate) fn definition_raw(self) -> Vec<u8> {
        let (bytes, width) = self.definition_token();
        bytes[..width].to_vec()
    }

    pub(crate) fn definition_token(self) -> ([u8; 2], usize) {
        if self.0 < 128 {
            ([self.0, 0], 1)
        } else {
            ([0x80, self.0 - 1], 2)
        }
    }

    pub(crate) fn display_raw(self) -> Vec<u8> {
        if self.0 < 128 {
            vec![self.0]
        } else {
            vec![0x80, self.0]
        }
    }

    pub(crate) fn read_display(raw: &[u8]) -> Option<Self> {
        match raw {
            [value @ 1..=127] | [0x80, value @ 128..=216] => Some(Self(*value)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorAtom {
    Zero,
    One,
    Shifted(ShiftedScalar),
}

/// Source atom whose normalized value lies in the closed unit interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColorComponent(ColorAtom);

impl ColorComponent {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let atom = match bytes.first()? {
            0 => ColorAtom::Zero,
            1 => ColorAtom::One,
            _ => ColorAtom::Shifted(ShiftedScalar::read(bytes)?),
        };
        if let ColorAtom::Shifted(scalar) = atom {
            if !(0.0..=1.0).contains(&(scalar.value() / 4.0)) {
                return None;
            }
        }
        Some(Self(atom))
    }

    pub(crate) fn value(self) -> f32 {
        match self.0 {
            ColorAtom::Zero => 0.0,
            ColorAtom::One => 1.0,
            ColorAtom::Shifted(scalar) => (scalar.value() / 4.0) as f32,
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            ColorAtom::Zero => &[0],
            ColorAtom::One => &[1],
            ColorAtom::Shifted(scalar) => scalar.raw(),
        }
    }

    pub(crate) fn from_wire(value: f32, raw: &[u8]) -> Result<Self, &'static str> {
        let component = Self::read(raw).ok_or("raw_components: invalid normalized color atom")?;
        if component.raw().len() != raw.len() {
            return Err("raw_components: trailing bytes after color atom");
        }
        if value.to_bits() != component.value().to_bits() {
            return Err("rgb: differs from raw_components");
        }
        Ok(component)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_index_distinguishes_definition_and_display_encodings() {
        for (value, definition, display) in [
            (1, vec![1], vec![1]),
            (127, vec![127], vec![127]),
            (128, vec![128, 127], vec![128, 128]),
            (216, vec![128, 215], vec![128, 216]),
        ] {
            let index = PaletteIndex::new(value).unwrap();
            assert_eq!(index.definition_raw(), definition);
            assert_eq!(index.display_raw(), display);
            assert_eq!(PaletteIndex::read_display(&display), Some(index));
        }
        assert!(PaletteIndex::new(0).is_none());
        assert!(PaletteIndex::new(217).is_none());
        assert!(PaletteIndex::read_display(&[128, 127]).is_none());
    }

    #[test]
    fn color_components_reject_out_of_range_atoms_and_trailing_wire_bytes() {
        for value in [-1.0_f64, 4.5] {
            let mut raw = value.to_be_bytes();
            raw[0] -= 0x10;
            assert!(ColorComponent::read(&raw).is_none());
        }
        assert!(ColorComponent::from_wire(1.0, &[1, 0]).is_err());
        assert!(ColorComponent::from_wire(0.0, &[1]).is_err());
    }
}
