// SPDX-License-Identifier: Apache-2.0
//! Admission of native, historical and generated body selections.

use super::{
    BodySelection, BodySelectionError, GeneratedBodyRef, NativeSelections, SelectionMembers,
    SewBodySelection,
};
use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

pub(super) fn known_body_count(selection: &BodySelection) -> Option<usize> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            Some(bodies.len())
        }
        BodySelection::Local { bodies, .. } => Some(bodies.len()),
        BodySelection::ResolvedSet { members } => Some(members.count()),
        BodySelection::Historical { bodies, .. } => Some(bodies.len()),
        BodySelection::HistoricalSet { members, .. } => Some(members.count()),
        BodySelection::Generated { bodies, .. } => Some(bodies.len()),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => None,
    }
}

impl TryFrom<BodySelection> for SewBodySelection {
    type Error = &'static str;
    fn try_from(bodies: BodySelection) -> Result<Self, Self::Error> {
        if known_body_count(&bodies).is_some_and(|count| count < 2) {
            return Err("bodies must select at least two bodies for sewing");
        }
        Ok(Self(bodies))
    }
}

impl BodySelection {
    /// Admit decoded local operands and their native reference.
    pub fn local(bodies: Vec<String>, native: String) -> Result<Self, BodySelectionError> {
        Ok(Self::Local {
            bodies: NativeSelections::try_from(bodies)?,
            native: native.try_into()?,
        })
    }

    /// Admit nonempty distinct historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        bodies: Vec<HistoricalBodyId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Historical {
            state,
            bodies: SelectionMembers::try_from(bodies)?,
            native: native.try_into()?,
        })
    }

    /// Admit nonempty distinct generated members and their native reference.
    pub fn generated(
        bodies: Vec<GeneratedBodyRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            bodies: SelectionMembers::try_from(bodies)?,
            native: native.try_into()?,
        })
    }
}

impl From<BodySelectionError> for cadmpeg_core::CodecError {
    fn from(error: BodySelectionError) -> Self {
        Self::malformed(error.to_string())
    }
}
