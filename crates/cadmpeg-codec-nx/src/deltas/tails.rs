// SPDX-License-Identifier: Apache-2.0
//! Complete null-reference trailers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NullTailForm {
    Two,
    Four,
}
impl NullTailForm {
    pub(crate) fn raw(self) -> &'static [u8] {
        match self {
            Self::Two => &[0, 1, 0, 1],
            Self::Four => &[0, 1, 0, 1, 0, 1, 0, 1],
        }
    }
    pub(crate) fn references(self) -> &'static [u32] {
        match self {
            Self::Two => &[1; 2],
            Self::Four => &[1; 4],
        }
    }
    pub(crate) fn from_references(references: &[u32]) -> Result<Self, &'static str> {
        match references {
            [1, 1] => Ok(Self::Two),
            [1, 1, 1, 1] => Ok(Self::Four),
            _ => Err("references: expected two or four null XMT references"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalNullReferences {
    offset: usize,
    form: NullTailForm,
}
impl TerminalNullReferences {
    pub(super) fn at_end(stream: &[u8]) -> Option<Self> {
        [NullTailForm::Four, NullTailForm::Two]
            .into_iter()
            .find_map(|form| {
                stream.ends_with(form.raw()).then(|| Self {
                    offset: stream.len() - form.raw().len(),
                    form,
                })
            })
    }
    pub(crate) fn offset(self) -> usize {
        self.offset
    }
    pub(super) fn end(self) -> usize {
        self.offset + self.form.raw().len()
    }
    pub(crate) fn form(self) -> NullTailForm {
        self.form
    }
}
