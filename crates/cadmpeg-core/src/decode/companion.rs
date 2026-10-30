// SPDX-License-Identifier: Apache-2.0
//! Files that a decoded input names by path.

use std::fmt::Debug;

use crate::CodecError;

use super::context::DecodeContext;
use super::view::{u64_from_index, View};

/// A file that a [`Companions`] source opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Companion {
    /// The identity of the file: equal for every path that names it, and
    /// the anchor of the paths that it names in turn.
    pub id: String,
    /// The content of the file.
    pub bytes: Vec<u8>,
}

/// A source of the files that a decoded input names by path.
pub trait Companions: Debug {
    /// Opens the file that `path` names, read relative to the file that
    /// `anchor` identifies, or to the decoded input when `anchor` is
    /// `None`. A file longer than `max_bytes` is refused. The error is the
    /// reason that the file is not available.
    fn open(&self, anchor: Option<&str>, path: &str, max_bytes: u64) -> Result<Companion, String>;

    /// The identity of the file that `path` names, read relative to
    /// `anchor` as [`Companions::open`] reads it, without reading the file.
    /// The identity is the one `open` gives. The default refuses: the
    /// source opens files only.
    fn locate(&self, _anchor: Option<&str>, _path: &str) -> Result<String, String> {
        Err("the companion file source does not locate files".to_owned())
    }
}

/// The result of a companion request.
#[derive(Debug, Clone)]
pub enum CompanionLoad<'a> {
    /// The opened file: its identity, and its content in a derived space.
    Loaded {
        /// The identity of the file.
        id: String,
        /// The content of the file.
        view: View<'a>,
    },
    /// The file is not available, for the reason given.
    Unavailable(String),
}

impl<'a> DecodeContext<'a> {
    /// Attaches the source of the files that the input names.
    #[must_use]
    pub fn with_companions(mut self, companions: &'a dyn Companions) -> Self {
        self.companions = Some(companions);
        self
    }

    /// Opens the file that `path` names, relative to the file that
    /// `anchor` identifies, charges its length to the input budget, and
    /// places its content in a derived space.
    pub fn open_companion(
        &self,
        anchor: Option<&str>,
        path: &str,
    ) -> Result<CompanionLoad<'a>, CodecError> {
        let Some(companions) = self.companions else {
            return Ok(CompanionLoad::Unavailable(
                "the decode has no companion file source".to_owned(),
            ));
        };
        let max = self
            .policy()
            .limits
            .max_input_bytes
            .checked_sub(self.budget.input_bytes())
            .ok_or_else(|| self.refuse_input_limit(0, "open companion file"))?;
        match companions.open(anchor, path, max) {
            Ok(companion) => {
                self.budget
                    .charge_input(u64_from_index(companion.bytes.len()), "open companion file")?;
                let bytes = self.arena.alloc(companion.bytes.into_boxed_slice());
                let space = self.allocate_space()?;
                Ok(CompanionLoad::Loaded {
                    id: companion.id,
                    view: View::over_space(bytes, space),
                })
            }
            Err(reason) => Ok(CompanionLoad::Unavailable(reason)),
        }
    }

    /// The identity of the file that `path` names, relative to the file that
    /// `anchor` identifies, without reading the file. The error is the
    /// reason that the file is not located.
    pub fn locate_companion(&self, anchor: Option<&str>, path: &str) -> Result<String, String> {
        match self.companions {
            Some(companions) => companions.locate(anchor, path),
            None => Err("the decode has no companion file source".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests;
