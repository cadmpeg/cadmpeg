// SPDX-License-Identifier: Apache-2.0
//! Parse-local source fields for assembly operand paths.

use crate::records::feature::assembly::DesignAssemblyOperandPath;
use cadmpeg_core::decode::View;

pub(super) struct ParsedOperandPath<'a> {
    pub(super) content: DesignAssemblyOperandPath,
    pub(super) source: PathSourceDraft<'a>,
}

pub(super) struct PathLinkSources<'a> {
    locator: View<'a>,
    wrapper: View<'a>,
}

/// These are the actual encodings accepted by the corresponding exact parser.
#[derive(Clone, Copy)]
pub(super) enum PathSourceForm {
    Counted,
    Legacy383,
    Legacy388,
    Carrier363,
}

impl<'a> PathLinkSources<'a> {
    pub(super) fn new(source: View<'a>, locator: usize, wrapper: usize) -> Option<Self> {
        Some(Self {
            locator: header(source, locator, 4)?,
            wrapper: header(source, wrapper, 4)?,
        })
    }
}

pub(super) struct PathSourceDraft<'a> {
    bulk: View<'a>,
    form: PathSourceForm,
    header: View<'a>,
    link: Option<PathLinkSources<'a>>,
    occurrences: Vec<View<'a>>,
    identities: Vec<View<'a>>,
    pairs: Vec<View<'a>>,
}

impl<'a> PathSourceDraft<'a> {
    pub(super) fn new(source: View<'a>, start: usize) -> Option<Self> {
        Some(Self {
            bulk: source,
            form: PathSourceForm::Counted,
            header: header(source, start, 8)?,
            link: None,
            occurrences: Vec::new(),
            identities: Vec::new(),
            pairs: Vec::new(),
        })
    }

    pub(super) fn legacy(source: View<'a>, start: usize, form: PathSourceForm) -> Option<Self> {
        let mut draft = Self::new(source, start)?;
        draft.form = form;
        draft.header = header(source, start, 4)?;
        Some(draft)
    }

    pub(super) fn locator_start(&self) -> Option<usize> {
        self.link
            .as_ref()?
            .locator
            .start()
            .checked_sub(self.bulk.start())
    }
    pub(super) fn wrapper_start(&self) -> Option<usize> {
        self.link
            .as_ref()?
            .wrapper
            .start()
            .checked_sub(self.bulk.start())
    }

    pub(super) fn validate_positions(&self) -> Option<()> {
        for fields in [&self.occurrences, &self.identities, &self.pairs] {
            if fields
                .windows(2)
                .any(|pair| pair[0].start() >= pair[1].start())
            {
                return None;
            }
        }
        if matches!(self.form, PathSourceForm::Counted)
            && self
                .occurrences
                .iter()
                .chain(&self.identities)
                .any(|field| field.start() <= self.header.start())
        {
            return None;
        }
        let first = self.occurrences.first()?;
        if !self.pairs.len().is_multiple_of(2)
            || self
                .pairs
                .iter()
                .any(|field| field.start() <= first.start())
        {
            return None;
        }
        Some(())
    }

    pub(super) fn set_link(&mut self, source: PathLinkSources<'a>) {
        self.link = Some(source);
    }

    pub(super) fn occurrence(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.occurrences.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn identity(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.identities.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn pair(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.pairs.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn prepend_occurrences(&mut self, preceding: Self) -> Option<()> {
        if !std::ptr::eq(self.bulk.window(), preceding.bulk.window())
            || self.bulk.start() != preceding.bulk.start()
            || self.bulk.end() != preceding.bulk.end()
        {
            return None;
        }
        let mut occurrences = preceding.occurrences;
        occurrences.append(&mut self.occurrences);
        self.occurrences = occurrences;
        self.validate_positions()
    }

    pub(super) fn append(&mut self, continuation: Self) -> Option<()> {
        if !std::ptr::eq(self.bulk.window(), continuation.bulk.window())
            || self.bulk.start() != continuation.bulk.start()
            || self.bulk.end() != continuation.bulk.end()
        {
            return None;
        }
        self.occurrences.extend(continuation.occurrences);
        self.identities.extend(continuation.identities);
        self.pairs.extend(continuation.pairs);
        self.validate_positions()
    }
}

fn field(source: View<'_>, relative: usize, len: usize) -> Option<View<'_>> {
    let start = source.start().checked_add(relative)?;
    source.child(start, start.checked_add(len)?)
}

fn header(source: View<'_>, relative: usize, index_bytes: usize) -> Option<View<'_>> {
    let len = usize::try_from(View::u32_le_at(source.window(), relative)?).ok()?;
    field(
        source,
        relative,
        4usize.checked_add(len)?.checked_add(index_bytes)?,
    )
}

fn text(source: View<'_>, count_at: usize, end: usize) -> Option<View<'_>> {
    field(source, count_at, end.checked_sub(count_at)?)
}
