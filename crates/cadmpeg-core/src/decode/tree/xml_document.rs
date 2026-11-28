// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;

use super::dtd::Prepared;

type Tree<'a> = roxmltree::Document<'a>;

self_cell::self_cell!(
    struct Recovered {
        owner: String,

        #[covariant]
        dependent: Tree,
    }
);

enum Storage<'input> {
    Borrowed(Tree<'input>),
    Recovered {
        document: Recovered,
        at: usize,
        inserted: usize,
    },
}

/// An admitted XML tree and its original source.
pub struct XmlDocument<'input> {
    source: &'input str,
    storage: Storage<'input>,
}

impl<'input> XmlDocument<'input> {
    /// Wraps a tree whose ranges already index its source.
    pub fn original(tree: Tree<'input>) -> Self {
        Self {
            source: tree.input_text(),
            storage: Storage::Borrowed(tree),
        }
    }

    pub(super) fn recovered<E>(
        source: &'input str,
        prepared: Prepared,
        parse: impl for<'a> FnOnce(&'a String) -> Result<Tree<'a>, E>,
    ) -> Result<Self, E> {
        let Prepared { text, at, inserted } = prepared;
        Ok(Self {
            source,
            storage: Storage::Recovered {
                document: Recovered::try_new(text, parse)?,
                at,
                inserted,
            },
        })
    }

    /// Returns the parsed tree. Its ranges index the text passed to the parser.
    pub fn tree(&self) -> &Tree<'_> {
        match &self.storage {
            Storage::Borrowed(tree) => tree,
            Storage::Recovered { document, .. } => document.borrow_dependent(),
        }
    }

    /// Returns the original XML text.
    pub fn source(&self) -> &'input str {
        self.source
    }

    /// Maps a parsed range back to the original XML text.
    pub fn source_range(&self, range: Range<usize>) -> Range<usize> {
        let Storage::Recovered { at, inserted, .. } = self.storage else {
            return range;
        };
        let position = |offset: usize| {
            if offset <= at {
                offset
            } else if offset < at + inserted {
                at
            } else {
                offset - inserted
            }
        };
        position(range.start)..position(range.end)
    }

    /// Returns the document element.
    pub fn root_element(&self) -> roxmltree::Node<'_, '_> {
        self.tree().root_element()
    }

    /// Iterates nodes in document order.
    pub fn descendants(&self) -> roxmltree::Descendants<'_, '_> {
        self.tree().descendants()
    }
}

impl std::fmt::Debug for XmlDocument<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.tree(), formatter)
    }
}
