// SPDX-License-Identifier: Apache-2.0
//! Borrowed structural views of native construction payloads.

use super::{CacheContract, CompoundComponent, CompoundCurveConstruction, LegacyCache, RevisionSurfaceForm, TSplineSubtransform, TSplineSurfaceConstruction};
use crate::ids::CurveId;
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;
use serde::{Serialize, Serializer};

impl Serialize for CompoundCurveConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            parameters: &'a [FiniteReal],
            components: &'a [CompoundComponent<CurveId, FiniteReal>],
            #[serde(skip_serializing_if = "Option::is_none")]
            cache: Option<LegacyCache>,
        }
        Wire { parameters: &self.parameters, components: &self.components, cache: self.cache }.serialize(serializer)
    }
}

impl Serialize for TSplineSurfaceConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            parameter_ranges: &'a [ParameterInterval; 2],
            type_code: i64,
            subtransform: &'a TSplineSubtransform,
            trailing_value: i64,
            discontinuities: &'a [Vec<FiniteReal>; 6],
            discontinuity_flag: bool,
            #[serde(skip_serializing_if = "CacheContract::is_bare_legacy")]
            cache: &'a CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
        }
        Wire { parameter_ranges: &self.parameter_ranges, type_code: self.type_code, subtransform: &self.subtransform, trailing_value: self.trailing_value, discontinuities: &self.discontinuities, discontinuity_flag: self.discontinuity_flag, cache: &self.cache }.serialize(serializer)
    }
}
