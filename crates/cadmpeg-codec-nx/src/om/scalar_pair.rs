// SPDX-License-Identifier: Apache-2.0
//! Closed discriminators for fixed scalar pairs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchPairForm { Legacy, Short, Extended, ThreeMember }

impl SketchPairForm {
    pub(crate) const ALL: [Self; 4] = [Self::Legacy, Self::Short, Self::Extended, Self::ThreeMember];
    pub(crate) fn discriminator(self) -> &'static [u8] {
    const LEGACY: [u8; 8] = [0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84];
    const SHORT: [u8; 15] = [
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x01,
    ];
    const EXTENDED: [u8; 17] = [
        0x08, 0x02, 0x03, 0x01, 0xc0, 0x40, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01,
    ];
    const THREE_MEMBER: [u8; 15] = [
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ];
        match self { Self::Legacy => &LEGACY, Self::Short => &SHORT, Self::Extended => &EXTENDED, Self::ThreeMember => &THREE_MEMBER }
    }
    pub(crate) fn separator_width(self) -> usize {
        match self { Self::Legacy | Self::ThreeMember => 1, Self::Short | Self::Extended => 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatumPairForm { Initial, Continuation }

impl DatumPairForm {
    pub(crate) const ALL: [Self; 2] = [Self::Initial, Self::Continuation];
    pub(crate) fn discriminator(self) -> &'static [u8] {
    const DISCRIMINATORS: [&[u8]; 2] = [
        &[
            0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
            0x03,
        ],
        &[
            0x80, 0x8d, 0x00, 0xff, 0x80, 0x81, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x87, 0xd7,
            0x01, 0x01, 0x01, 0x01, 0x02, 0xa5, 0x30, 0x21, 0xa5, 0x30, 0x21, 0x01, 0x00, 0x01,
            0xaf, 0xff, 0xdf, 0x02, 0x01, 0x02,
        ],
    ];
        match self { Self::Initial => DISCRIMINATORS[0], Self::Continuation => DISCRIMINATORS[1] }
    }
}

/// Closed framing contract for two scalar atoms.
pub(crate) trait PairForm: Copy + 'static {
    const ALL_FORMS: &'static [Self];
    fn prefix(self) -> &'static [u8];
    fn second_delta(self) -> u64;
}
impl PairForm for SketchPairForm {
    const ALL_FORMS: &'static [Self] = &Self::ALL;
    fn prefix(self) -> &'static [u8] { self.discriminator() }
    fn second_delta(self) -> u64 { 8 + self.separator_width() as u64 }
}
impl PairForm for DatumPairForm {
    const ALL_FORMS: &'static [Self] = &Self::ALL;
    fn prefix(self) -> &'static [u8] { self.discriminator() }
    fn second_delta(self) -> u64 { 9 }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MixedPairForm;
impl PairForm for MixedPairForm {
    const ALL_FORMS: &'static [Self] = &[Self];
    fn prefix(self) -> &'static [u8] { SketchPairForm::Legacy.discriminator() }
    fn second_delta(self) -> u64 { 9 }
}

/// Pair position whose derived atom offsets fit the payload address space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairPosition<F> {
    form: F,
    offset: u64,
}
impl<F: PairForm> PairPosition<F> {
    pub(crate) fn new(form: F, offset: u64) -> Option<Self> {
        offset.checked_add(form.prefix().len() as u64)?.checked_add(form.second_delta())?;
        Some(Self { form, offset })
    }
    pub(crate) fn from_wire(discriminator: &[u8], offset: u64, values: [u64; 2]) -> Result<Self, String> {
        let form = F::ALL_FORMS.iter().copied().find(|form| form.prefix() == discriminator)
            .ok_or("discriminator: unsupported scalar-pair framing")?;
        let position = Self::new(form, offset).ok_or("payload_offset: scalar-pair offsets overflow")?;
        if position.value_offsets() != values {
            return Err("value_payload_offsets: differ from scalar-pair framing".into());
        }
        Ok(position)
    }
    pub(crate) fn form(self) -> F { self.form }
    pub(crate) fn discriminator(self) -> &'static [u8] { self.form.prefix() }
    pub(crate) fn offset(self) -> u64 { self.offset }
    pub(crate) fn value_offsets(self) -> [u64; 2] {
        let first = self.offset + self.form.prefix().len() as u64;
        [first, first + self.form.second_delta()]
    }
}
