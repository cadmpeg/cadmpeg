// SPDX-License-Identifier: Apache-2.0
//! Output selection for record-oriented queries.

use clap::Args;

/// Shared output arguments for record-oriented queries.
#[derive(Debug, Args)]
pub(crate) struct OutputArgs {
    /// Comma-separated dotted field paths relative to each result; project as TSV.
    #[arg(long, value_delimiter = ',', conflicts_with = "json")]
    fields: Option<Vec<String>>,
    /// Wrap results in the JSON envelope.
    #[arg(long)]
    json: bool,
}

impl OutputArgs {
    /// Resolves command-line output arguments into one output mode.
    pub(crate) fn mode(&self) -> Output<'_> {
        if self.json {
            Output::Json
        } else if let Some(paths) = self.fields.as_deref() {
            Output::Tsv(paths)
        } else {
            Output::Pretty
        }
    }
}

/// Record-oriented query output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output<'a> {
    Pretty,
    Tsv(&'a [String]),
    Json,
}
