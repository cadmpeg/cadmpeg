// SPDX-License-Identifier: Apache-2.0
//! Typed scalar frames with contiguous, derived payload positions.

use super::fixed::Q155Atom;
use super::nonempty::NonEmpty;
use super::scalar::{ShiftedBinary32, ShiftedScalar};

pub(crate) trait AtomWidth {
    fn width(&self) -> u64;
}

impl AtomWidth for ShiftedScalar {
    fn width(&self) -> u64 {
        cadmpeg_core::decode::u64_from_index(self.raw().len())
    }
}

impl AtomWidth for ShiftedBinary32 {
    fn width(&self) -> u64 {
        4
    }
}

impl AtomWidth for Q155Atom {
    fn width(&self) -> u64 {
        8
    }
}

pub(crate) trait ScalarFrame: Copy + std::fmt::Debug + Eq {
    type Atom: AtomWidth + Clone + std::fmt::Debug + Eq;
    fn prefix_len(self) -> u64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramedScalarRun<F: ScalarFrame, O> {
    form: F,
    offset: u64,
    values: NonEmpty<(F::Atom, O)>,
    end: u64,
}

impl<F: ScalarFrame, O> FramedScalarRun<F, O> {
    pub(crate) fn new(
        form: F,
        offset: u64,
        values: NonEmpty<(F::Atom, O)>,
    ) -> Result<Self, &'static str> {
        let end = match Self::validate(form, offset, &values, |values| {
            Ok::<_, std::convert::Infallible>(values.iter())
        }) {
            Ok(end) => end?,
            Err(error) => match error {},
        };
        Ok(Self { form, offset, values, end })
    }

    pub(crate) fn from_wire(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        form: F, offset: u64, values: NonEmpty<(F::Atom, O)>,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        Ok(Self::validate(form, offset, &values, |values| {
            ctx.admit_iter(values, "NX scalar run token widths")
        })?.map(|end| Self { form, offset, values, end }))
    }

    fn validate<'a, E, I: Iterator<Item = &'a (F::Atom, O)>>(
        form: F, offset: u64, values: &'a NonEmpty<(F::Atom, O)>,
        mut admit: impl FnMut(&'a [(F::Atom, O)]) -> Result<I, E>,
    ) -> Result<Result<u64, &'static str>, E> where F::Atom: 'a, O: 'a {
        let Some(start) = offset.checked_add(form.prefix_len()) else {
            return Ok(Err("value_payload_offsets overflow the discriminator"));
        };
        let end = admit(values.initial())?
            .chain(admit(std::slice::from_ref(values.last()))?)
            .try_fold(start, |at, (atom, _)| at.checked_add(atom.width()));
        Ok(end.ok_or("value_payload_offsets overflow the scalar run"))
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn form(&self) -> F {
        self.form
    }

    pub(crate) fn end(&self) -> u64 {
        self.end
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &F::Atom, &O)> + Clone {
        let mut at = self.offset + self.form.prefix_len();
        self.values.iter().map(move |(atom, location)| {
            let offset = at;
            at += atom.width();
            (offset, atom, location)
        })
    }

    pub(crate) fn try_map_locations<P>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut map: impl FnMut(u64, O) -> Option<P>,
    ) -> Result<Option<FramedScalarRun<F, P>>, cadmpeg_core::CodecError> {
        let mut at = self.offset + self.form.prefix_len();
        let values = self.values.try_map_charged(ctx, |(atom, location)| {
            let offset = at;
            at += atom.width();
            Some((atom, map(offset, location)?))
        })?;
        let Some(values) = values else {
            return Ok(None);
        };
        Ok(Some(FramedScalarRun {
            form: self.form,
            offset: self.offset,
            values,
            end: self.end,
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scalar_run_width_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[], ResourceDimension::WorkUnits, "NX scalar run token widths",
            |ctx| {
                let atom = crate::om::fixed::Q155Atom {
                    marker: crate::om::fixed::Q155Marker::M30,
                    scalar: crate::om::fixed::Q155::from_raw([0; 7]).unwrap(),
                };
                let values = crate::om::nonempty::NonEmpty::from_admitted_vec(vec![(atom, ())]).unwrap();
                super::FramedScalarRun::from_wire(ctx, crate::om::fixed::Q155LaneFrame, 0, values)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX scalar run token widths"));
    }
}
