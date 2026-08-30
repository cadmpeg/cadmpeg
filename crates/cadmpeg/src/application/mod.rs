// SPDX-License-Identifier: Apache-2.0
//! Typed application workflows for the `cadmpeg` CLI.

pub mod artifact_store;
pub mod document;
pub mod refusal;
pub mod transcoder;
pub mod validators;

pub use artifact_store::{ArtifactStore, SidecarPersistOutcome};
pub use document::{LoadOrigin, LoadedDocument};
pub use refusal::ConversionRefusal;
pub use transcoder::{
    export_target, ConversionPolicy, DestinationPolicy, LossPolicy, SourceRequest, Transcoder,
    ValidationAdmission,
};
pub use validators::NativeValidatorCatalog;
