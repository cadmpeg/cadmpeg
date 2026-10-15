// SPDX-License-Identifier: Apache-2.0
//! Admission of native, historical and generated body selections.

use super::{
    BodySelection, BodySelectionError, GeneratedBodyRef, NativeSelections, SelectionMembers,
};
use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

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
