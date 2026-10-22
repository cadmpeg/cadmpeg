// SPDX-License-Identifier: Apache-2.0
//! Parameter-state links and their document-root headers.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::schema::rewrite::typed::{
    prune::{DependencyPosition, DependencyReference},
    RewriteIdentities,
};
use serde::{Deserialize, Serialize};

use super::references::DesignClassTag;

/// CommonData version-4 state selected by a typed parameter tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignParameterState {
    /// Identity of the actual primary record.
    pub(crate) id: String,
    /// Segment-local class registration.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed entity identity.
    pub(crate) record_index: u32,
    /// Required reciprocal state companion.
    pub(crate) companion: String,
    /// Required Fusion document-root header.
    pub(crate) document_root: String,
}

/// CommonData version-1 companion of a parameter state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignParameterStateCompanion {
    /// Identity of the actual primary record.
    pub(crate) id: String,
    /// Segment-local class registration.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed entity identity.
    pub(crate) record_index: u32,
    /// State-control byte.
    pub(crate) control: u8,
    /// State-control word.
    pub(crate) control_word: u32,
    /// Required reciprocal parameter state.
    pub(crate) state: String,
}

/// Indexed header of the Fusion document root; its members are separate content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignParameterDocumentHeader {
    /// Identity of the actual root header.
    pub(crate) id: String,
    /// Segment-local class registration.
    pub(crate) class_tag: DesignClassTag,
    /// Indexed entity identity.
    pub(crate) record_index: u32,
}

macro_rules! state_identity {
    ($owner:ty; $($field:ident),+ $(,)?) => {
        impl RewriteIdentities for $owner {
            fn visit_identity_references(&self, _ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
                $(visitor(&self.$field)?;)*
                Ok(())
            }
            fn visit_dependency_references(&self, _ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>) -> Result<(), CodecError> {
                $(visitor(DependencyReference { target: &self.$field, position: DependencyPosition::Required, field: Some(stringify!($field)) })?;)*
                Ok(())
            }
            fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(mut self, _ctx: &DecodeContext<'_>, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>) -> Result<Self, CodecError> {
                self.id = map.identity(&self.id)?;
                $(self.$field = map.identity(&self.$field)?;)*
                Ok(self)
            }
            fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(_ctx: &DecodeContext<'_>, value: &mut serde_json::Value, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>) -> Result<(), CodecError> {
                for field in ["id", $(stringify!($field)),*] {
                    let id = value.get(field).and_then(serde_json::Value::as_str).ok_or_else(|| CodecError::malformed("parameter state reference must be a string"))?;
                    let replacement = map.identity(id)?;
                    value[field] = replacement.into();
                }
                Ok(())
            }
        }
    };
}
state_identity!(DesignParameterState; companion, document_root);
state_identity!(DesignParameterStateCompanion; state);
rewrite_native_identity_record!(DesignParameterDocumentHeader;);
