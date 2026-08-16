// SPDX-License-Identifier: Apache-2.0
//! Typed application workflows for the `cadmpeg` CLI.

pub mod artifact_store;
pub mod catalogs;
pub mod document;
pub mod encoders;
pub mod refusal;
pub mod transcoder;

pub use artifact_store::{ArtifactStore, SidecarPersistOutcome};
pub use catalogs::{
    ForcedInput, InputCatalog, NativeValidatorCatalog, ResolveSourceError, ResolvedSource,
};
pub use document::{LoadOrigin, LoadedDocument};
pub use encoders::{build_encoder, EncoderRequest};
pub use refusal::ConversionRefusal;
pub use transcoder::{export_target, ConversionPolicy, SourceRequest, Transcoder};
