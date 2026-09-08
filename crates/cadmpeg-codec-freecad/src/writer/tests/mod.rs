// SPDX-License-Identifier: Apache-2.0
//! `FCStd` writer unit tests.

mod patching;
mod targets;

pub(crate) use patching::writer_rejects_unserialized_declaration_and_stale_payload_edits;
pub(crate) use targets::write_target_and_source_requirements_are_explicit;
