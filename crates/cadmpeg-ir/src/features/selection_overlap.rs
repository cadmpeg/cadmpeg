// SPDX-License-Identifier: Apache-2.0
//! Borrowed overlap checks for feature operand memberships.

use super::{
    BodyMembers, BodySelection, FaceSelection, GeneratedBodyRef, GeneratedFaceRef, VertexSelection,
};
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId, HistoricalFaceId};

fn any_overlap<'a, 'b, T: 'a + 'b>(
    first: impl IntoIterator<Item = &'a T>,
    second: &(impl Iterator<Item = &'b T> + Clone),
    equal: impl Fn(&T, &T) -> bool,
) -> bool {
    first
        .into_iter()
        .any(|first| second.clone().any(|second| equal(first, second)))
}

pub(super) fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    fn direct(selection: &FaceSelection) -> Option<&[crate::ids::FaceId]> {
        match selection {
            FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
                Some(faces.as_slice())
            }
            _ => None,
        }
    }
    fn historical(
        selection: &FaceSelection,
    ) -> Option<(&FeatureInputTopologyId, &[HistoricalFaceId])> {
        match selection {
            FaceSelection::Historical { state, faces, .. } => Some((state, faces.as_slice())),
            FaceSelection::HistoricalPartial { state, faces, .. } => {
                Some((state, faces.as_slice()))
            }
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return any_overlap(first, &second.iter(), |first, second| {
            first.as_str() == second.as_str()
        });
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        if first_state.as_str() != second_state.as_str() {
            return false;
        }
        return any_overlap(first, &second.iter(), |first, second| {
            first.as_str() == second.as_str()
        });
    }
    match (first, second) {
        (
            FaceSelection::Generated { faces: first, .. },
            FaceSelection::Generated { faces: second, .. },
        ) => any_overlap(first, &second.iter(), |first: &GeneratedFaceRef, second| {
            first.feature.as_str() == second.feature.as_str()
                && first.local_id.as_str() == second.local_id.as_str()
        }),
        _ => false,
    }
}

pub(super) fn vertex_targets_equal(first: &VertexSelection, second: &VertexSelection) -> bool {
    match (first, second) {
        (
            VertexSelection::Generated { vertex: first, .. },
            VertexSelection::Generated { vertex: second, .. },
        ) => {
            first.feature.as_str() == second.feature.as_str()
                && first.local_id.as_str() == second.local_id.as_str()
        }
        (
            VertexSelection::Historical {
                state: first_state,
                vertex: first_vertex,
                ..
            },
            VertexSelection::Historical {
                state: second_state,
                vertex: second_vertex,
                ..
            },
        ) => {
            first_state.as_str() == second_state.as_str()
                && first_vertex.as_str() == second_vertex.as_str()
        }
        (VertexSelection::Native(first), VertexSelection::Native(second)) => {
            first.as_str() == second.as_str()
        }
        (VertexSelection::Unresolved, VertexSelection::Unresolved) => true,
        _ => false,
    }
}

/// Flat and paired rows expose the same borrowed identity sequence.
enum BodyRows<'a, T> {
    Flat(&'a [T]),
    Paired(&'a BodyMembers<T>),
}

impl<T> Copy for BodyRows<'_, T> {}
impl<T> Clone for BodyRows<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> BodyRows<'a, T> {
    fn iter(&self) -> impl Iterator<Item = &'a T> + Clone {
        let rows = *self;
        let count = match rows {
            Self::Flat(values) => values.len(),
            Self::Paired(values) => values.count(),
        };
        (0..count).map(move |index| match rows {
            Self::Flat(values) => &values[index],
            Self::Paired(values) => values.0[index].body(),
        })
    }
}

pub(super) fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    fn direct(selection: &BodySelection) -> Option<BodyRows<'_, BodyId>> {
        match selection {
            BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
                Some(BodyRows::Flat(bodies.as_slice()))
            }
            BodySelection::ResolvedSet { members } => Some(BodyRows::Paired(members)),
            _ => None,
        }
    }
    fn historical(
        selection: &BodySelection,
    ) -> Option<(&FeatureInputTopologyId, BodyRows<'_, HistoricalBodyId>)> {
        match selection {
            BodySelection::Historical { state, bodies, .. } => {
                Some((state, BodyRows::Flat(bodies.as_slice())))
            }
            BodySelection::HistoricalSet { state, members } => {
                Some((state, BodyRows::Paired(members)))
            }
            _ => None,
        }
    }
    if let Some(first) = direct(first) {
        let Some(second) = direct(second) else {
            return false;
        };
        return any_overlap(first.iter(), &second.iter(), |first, second| {
            first.as_str() == second.as_str()
        });
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        if first_state.as_str() != second_state.as_str() {
            return false;
        }
        return any_overlap(first.iter(), &second.iter(), |first, second| {
            first.as_str() == second.as_str()
        });
    }
    match (first, second) {
        (
            BodySelection::Generated { bodies: first, .. },
            BodySelection::Generated { bodies: second, .. },
        ) => any_overlap(first, &second.iter(), |first: &GeneratedBodyRef, second| {
            first.feature.as_str() == second.feature.as_str()
                && first.local_id.as_str() == second.local_id.as_str()
        }),
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => any_overlap(first.as_slice(), &second.iter(), |first, second| {
            first == second
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
