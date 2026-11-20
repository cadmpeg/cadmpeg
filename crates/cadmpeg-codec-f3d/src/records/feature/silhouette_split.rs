// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_carrier, DesignSilhouetteDirection, "carrier");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSilhouetteSplit {
    pub(crate) mode_word: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier"
    )]
    pub(crate) carrier: Option<DesignSilhouetteDirection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSilhouetteDirection {
    pub(crate) position: [FiniteReal; 3],
    pub(crate) direction: [FiniteReal; 3],
    pub(crate) reference_type: u32,
}

rewrite_native_record!(DesignSilhouetteSplit, []; { mode_word, carrier });
rewrite_native_record!(DesignSilhouetteDirection, []; { position, direction, reference_type });
