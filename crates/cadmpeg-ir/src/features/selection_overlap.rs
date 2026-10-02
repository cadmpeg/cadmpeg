// SPDX-License-Identifier: Apache-2.0
//! Borrowed, fallible overlap checks for feature operand memberships.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit};

use super::{BodyMembers, BodySelection, FaceSelection, GeneratedBodyRef, GeneratedFaceRef};
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId, HistoricalFaceId};

pub(super) trait OverlapAdmission {
    type Error;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
}

pub(super) struct StandardAdmission;

impl OverlapAdmission for StandardAdmission {
    type Error = std::convert::Infallible;
    fn work(&self, _count: usize) -> Result<(), Self::Error> { Ok(()) }
}

impl OverlapAdmission for DecodeContext<'_> {
    type Error = ResourceLimit;
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.charge_work_limit(u64_from_index(count), "IR selection membership overlap")
    }
}

pub(super) fn standard_result<T>(value: Result<T, std::convert::Infallible>) -> T {
    match value {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

fn text_equal<S: OverlapAdmission>(admission: &S, first: &str, second: &str) -> Result<bool, S::Error> {
    admission.work(1)?;
    if first.len() != second.len() { return Ok(false); }
    admission.work(first.len())?;
    Ok(first == second)
}

fn any_overlap<'a, 'b, S: OverlapAdmission, T: 'a + 'b>(
    admission: &S,
    first: impl IntoIterator<Item = &'a T>,
    second: impl Iterator<Item = &'b T> + Clone,
    equal: impl Fn(&T, &T) -> Result<bool, S::Error>,
) -> Result<bool, S::Error> {
    for first in first {
        admission.work(1)?;
        for second in second.clone() {
            admission.work(1)?;
            if equal(first, second)? { return Ok(true); }
        }
    }
    Ok(false)
}

pub(super) fn face_selections_overlap<S: OverlapAdmission>(
    admission: &S,
    first: &FaceSelection,
    second: &FaceSelection,
) -> Result<bool, S::Error> {
    fn direct(selection: &FaceSelection) -> Option<&[crate::ids::FaceId]> {
        match selection {
            FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => Some(faces.as_slice()),
            _ => None,
        }
    }
    fn historical(selection: &FaceSelection) -> Option<(&FeatureInputTopologyId, &[HistoricalFaceId])> {
        match selection {
            FaceSelection::Historical { state, faces, .. } => Some((state, faces.as_slice())),
            FaceSelection::HistoricalPartial { state, faces, .. } => Some((state, faces.as_slice())),
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return any_overlap(admission, first, second.iter(), |first, second| text_equal(admission, first.as_str(), second.as_str()));
    }
    if let Some(((first_state, first), (second_state, second))) = historical(first).zip(historical(second)) {
        if !text_equal(admission, first_state.as_str(), second_state.as_str())? { return Ok(false); }
        return any_overlap(admission, first, second.iter(), |first, second| text_equal(admission, first.as_str(), second.as_str()));
    }
    match (first, second) {
        (FaceSelection::Generated { faces: first, .. }, FaceSelection::Generated { faces: second, .. }) => {
            any_overlap(admission, first, second.iter(), |first: &GeneratedFaceRef, second| {
                Ok(text_equal(admission, first.feature.as_str(), second.feature.as_str())?
                    && text_equal(admission, first.local_id.as_str(), second.local_id.as_str())?)
            })
        }
        _ => Ok(false),
    }
}

/// Flat and paired rows expose the same borrowed identity sequence.
enum BodyRows<'a, T> {
    Flat(&'a [T]),
    Paired(&'a BodyMembers<T>),
}

impl<T> Copy for BodyRows<'_, T> {}
impl<T> Clone for BodyRows<'_, T> {
    fn clone(&self) -> Self { *self }
}

impl<'a, T> BodyRows<'a, T> {
    fn iter(&self) -> impl Iterator<Item = &'a T> + Clone {
        let rows = *self;
        let count = match rows { Self::Flat(values) => values.len(), Self::Paired(values) => values.count() };
        (0..count).map(move |index| match rows {
            Self::Flat(values) => &values[index],
            Self::Paired(values) => values.0[index].body(),
        })
    }
}

pub(super) fn body_selections_overlap<S: OverlapAdmission>(
    admission: &S,
    first: &BodySelection,
    second: &BodySelection,
) -> Result<bool, S::Error> {
    fn direct(selection: &BodySelection) -> Option<BodyRows<'_, BodyId>> {
        match selection {
            BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => Some(BodyRows::Flat(bodies.as_slice())),
            BodySelection::ResolvedSet { members } => Some(BodyRows::Paired(members)),
            _ => None,
        }
    }
    fn historical(selection: &BodySelection) -> Option<(&FeatureInputTopologyId, BodyRows<'_, HistoricalBodyId>)> {
        match selection {
            BodySelection::Historical { state, bodies, .. } => Some((state, BodyRows::Flat(bodies.as_slice()))),
            BodySelection::HistoricalSet { state, members } => Some((state, BodyRows::Paired(members))),
            _ => None,
        }
    }
    if let Some(first) = direct(first) {
        let Some(second) = direct(second) else { return Ok(false); };
        return any_overlap(admission, first.iter(), second.iter(), |first, second| text_equal(admission, first.as_str(), second.as_str()));
    }
    if let Some(((first_state, first), (second_state, second))) = historical(first).zip(historical(second)) {
        if !text_equal(admission, first_state.as_str(), second_state.as_str())? { return Ok(false); }
        return any_overlap(admission, first.iter(), second.iter(), |first, second| text_equal(admission, first.as_str(), second.as_str()));
    }
    match (first, second) {
        (BodySelection::Generated { bodies: first, .. }, BodySelection::Generated { bodies: second, .. }) => {
            any_overlap(admission, first, second.iter(), |first: &GeneratedBodyRef, second| {
                Ok(text_equal(admission, first.feature.as_str(), second.feature.as_str())?
                    && text_equal(admission, first.local_id.as_str(), second.local_id.as_str())?)
            })
        }
        (BodySelection::Local { bodies: first, .. }, BodySelection::Local { bodies: second, .. }) => {
            any_overlap(admission, first.as_slice(), second.iter(), |first, second| text_equal(admission, first, second))
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests;
