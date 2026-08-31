// SPDX-License-Identifier: Apache-2.0
//! Runtime projections of the joined dialect registries.

use cadmpeg_core::dialect::{DialectId, DialectLayers};
use cadmpeg_ir::codec::TargetDescriptor;

use crate::disposition::ReadDisposition;
use crate::registry::{catalog_of, registries, support, DialectEntry};
use crate::{Format, InputCatalog};

/// What `cadmpeg inspect` knows about the dialect it matched.
///
/// Three sources in one value: the classifier's own primary-layer match, the
/// read disposition the capability registry records for that id, and the write
/// targets this build's encoder for that format can synthesize. How they are
/// rendered belongs to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectProvenance {
    /// The matched id.
    pub id: DialectId,
    /// The declared read disposition for that id, when the registry has one.
    pub read: Option<ReadDisposition>,
    /// The target ids this build can synthesize for the format, in catalog
    /// order. Empty when the build has no encoder for it, or the encoder has
    /// no catalog.
    pub write_targets: Vec<&'static str>,
}

/// The provenance of the primary dialect the codec matched.
///
/// Returns `None` when the codec reported no dialects at all, which is the
/// honest answer for a codec that does not classify.
#[must_use]
pub fn dialect_provenance(dialects: Option<&DialectLayers>) -> Option<DialectProvenance> {
    let entry = dialects?.primary();
    Some(DialectProvenance {
        id: entry.dialect().clone(),
        read: support(entry.dialect()).map(|disposition| disposition.read),
        write_targets: catalog_of(entry.format())
            .unwrap_or(&[])
            .iter()
            .map(|target| target.id)
            .collect(),
    })
}

/// One row of the format table: what this build does with one readable format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatRow {
    /// The format id.
    pub id: &'static str,
    /// Whether this build writes it.
    pub write: bool,
    /// The extensions the detector accepts for it.
    pub extensions: &'static [&'static str],
}

/// The readable formats in this build, with their write capability, in catalog
/// order.
#[must_use]
pub fn format_rows(inputs: &InputCatalog) -> Vec<FormatRow> {
    inputs
        .descriptors()
        .map(|descriptor| {
            let id = descriptor.format_id();
            FormatRow {
                // Every input descriptor is readable. CADIR carries no codec
                // because the neutral document is parsed, not decoded.
                id,
                write: Format::from_name(id).is_some(),
                extensions: descriptor.extensions,
            }
        })
        .collect()
}

/// A `format` argument that names no section of the identity registry.
#[derive(Debug, thiserror::Error)]
#[error("no format {name} in the dialect registry; known: {known}")]
pub struct UnknownFormat {
    /// The word the caller passed.
    name: String,
    /// The format ids the registry declares.
    known: String,
}

/// Every declared dialect of one format, with this build's write catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDialects {
    /// The format id.
    pub format: String,
    /// `None` when this build has no encoder for the format; otherwise the
    /// encoder's catalog, which is empty for an encoder with no dialects.
    pub catalog: Option<&'static [TargetDescriptor]>,
    /// The catalog's default target id, when it declares one.
    pub default_target: Option<&'static str>,
    /// The declared dialects, in registry order.
    pub rows: Vec<DialectEntry>,
}

/// The identity registry crossed with the capability registry.
///
/// `format` selects one section; `None` returns every one. The word is
/// resolved through [`Format::from_name`] first, so an output-format spelling
/// and a registry section name reach the same rows.
pub fn dialect_table(format: Option<&str>) -> Result<Vec<FormatDialects>, UnknownFormat> {
    let registries = registries();
    let formats = match format {
        None => registries.formats.clone(),
        Some(name) => {
            let name =
                Format::from_name(name).map_or_else(|| name.to_owned(), |f| f.name().to_owned());
            if !registries.formats.contains(&name) {
                return Err(UnknownFormat {
                    name,
                    known: registries.formats.join(", "),
                });
            }
            vec![name]
        }
    };

    Ok(formats
        .into_iter()
        .map(|name| {
            let catalog = catalog_of(&name);
            FormatDialects {
                catalog,
                default_target: catalog
                    .and_then(|targets| targets.iter().find(|target| target.default))
                    .map(|target| target.id),
                rows: registries.rows_of(&name).cloned().collect(),
                format: name,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::dialect::DialectMatch;

    #[cfg(feature = "rhino")]
    #[test]
    fn the_provenance_joins_the_match_the_registry_and_the_catalog() {
        let dialects = DialectLayers::of(DialectMatch::admitted(DialectId::pinned(
            "rhino:archive-50",
        )));
        let provenance = dialect_provenance(Some(&dialects)).expect("a primary layer exists");
        assert_eq!(provenance.id.as_str(), "rhino:archive-50");
        assert!(provenance.read.is_some());
        assert!(provenance.write_targets.contains(&"rhino:archive-50"));
        assert!(provenance.write_targets.contains(&"rhino:archive-80"));
    }

    #[test]
    fn no_dialects_is_no_provenance() {
        assert!(dialect_provenance(None).is_none());
    }

    #[test]
    fn the_dialect_table_selects_one_format_or_every_one() {
        assert!(dialect_table(Some("nonesuch")).is_err());
        let all = dialect_table(None).expect("every declared format");
        assert!(all.len() > 1);
        let one = dialect_table(Some(&all[0].format)).expect("a declared format");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rows.len(), all[0].rows.len());
    }

    #[test]
    fn format_aliases_reach_the_same_dialect_rows() {
        let canonical = crate::dialects("rhino")
            .into_iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        let alias = crate::dialects("3dm")
            .into_iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(alias, canonical);
        assert_eq!(
            dialect_table(Some("3dm")).expect("Rhino alias"),
            dialect_table(Some("rhino")).expect("Rhino format")
        );
    }
}
