// SPDX-License-Identifier: Apache-2.0
//! Exact Appendix C row admission and parent relationships.

use std::collections::BTreeMap;

use cadmpeg_ir::source_fidelity::parts::{PartFraming, PartLevel};

const HEADER: &str = "codec\tlevel\tparent\tframing\tend_rule\tframing_bytes\tdispatch_bytes\tlocator\tscan_recovery\tarenas\tcitation";

/// A declared parent alternative in Appendix C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartParent {
    /// A physical file or a shared root hosted by an embedding part.
    File,
    /// One exact qualified table level.
    Level(PartLevel),
}

/// One immutable table row, with qualified identities checked by the IR registry.
#[derive(Debug)]
pub struct PartRule<'a> {
    level: PartLevel,
    parents: Vec<PartParent>,
    end_rule: &'a str,
    framing_bytes: &'a str,
    dispatch_bytes: &'a str,
    locator: &'a str,
    scan_recovery: &'a str,
    arenas: &'a str,
    citation: &'a str,
}

impl<'a> PartRule<'a> {
    /// Exact qualified Appendix C identity.
    pub fn level(&self) -> PartLevel {
        self.level
    }

    /// All declared parent alternatives, without flattening `|`.
    pub fn parents(&self) -> &[PartParent] {
        &self.parents
    }

    /// The table's complete extent rule.
    pub fn end_rule(&self) -> &'a str {
        self.end_rule
    }

    /// The table's exact description of framing bytes.
    pub fn framing_bytes(&self) -> &'a str {
        self.framing_bytes
    }

    /// The table's exact description of content dispatch bytes.
    pub fn dispatch_bytes(&self) -> &'a str {
        self.dispatch_bytes
    }

    /// The table's locator rule.
    pub fn locator(&self) -> &'a str {
        self.locator
    }

    /// The table's recovery rule, including its stated conditions.
    pub fn scan_recovery(&self) -> &'a str {
        self.scan_recovery
    }

    /// Native arena names recorded in the table.
    pub fn arenas(&self) -> &'a str {
        self.arenas
    }

    /// The table's specification and decoder citation.
    pub fn citation(&self) -> &'a str {
        self.citation
    }

    /// An extent boundary that does not permit independent recovery.
    pub fn extent_only(&self) -> bool {
        self.end_rule.starts_with("extent only:")
    }
}

/// Strictly parsed Appendix C metadata.
#[derive(Debug)]
pub struct PartTable<'a> {
    rows: BTreeMap<&'static str, PartRule<'a>>,
}

impl PartTable<'static> {
    /// Load the authorized table embedded without transformations.
    pub fn load() -> Result<Self, String> {
        Self::parse(include_str!("../../../../scripts/part-framing.tsv"))
    }
}

impl<'a> PartTable<'a> {
    /// Parse table metadata; every row and parent must name a registered level.
    pub(super) fn parse(source: &'a str) -> Result<Self, String> {
        let mut lines = source.lines();
        if lines.next() != Some(HEADER) {
            return Err("Appendix C header differs from the eleven-column contract".into());
        }
        let mut rows = BTreeMap::new();
        for (index, line) in lines.enumerate() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 11 || fields.iter().any(|field| field.trim().is_empty()) {
                return Err(format!("Appendix C row {} has missing columns", index + 2));
            }
            let name = format!("{}.{}", fields[0], fields[1]);
            let level = PartLevel::parse(&name)
                .ok_or_else(|| format!("unregistered Appendix C level {name}"))?;
            let framing = match fields[3] {
                "located" => PartFraming::Located,
                "terminated" => PartFraming::Terminated,
                "sequential" => PartFraming::Sequential,
                other => return Err(format!("invalid framing {other} for {name}")),
            };
            if framing != level.framing() {
                return Err(format!("registry framing differs for {name}"));
            }
            if fields[4].starts_with("extent only:") && framing != PartFraming::Sequential {
                return Err(format!("extent-only level {name} is not sequential"));
            }
            let parents = fields[2]
                .split('|')
                .map(|parent| {
                    if parent == "file" {
                        Ok(PartParent::File)
                    } else {
                        PartLevel::parse(parent)
                            .map(PartParent::Level)
                            .ok_or_else(|| format!("unregistered parent {parent} for {name}"))
                    }
                })
                .collect::<Result<Vec<_>, String>>()?;
            for (position, parent) in parents.iter().enumerate() {
                if parents[..position].contains(parent) {
                    return Err(format!("duplicate parent for {name}"));
                }
            }
            let row = PartRule {
                level,
                parents,
                end_rule: fields[4],
                framing_bytes: fields[5],
                dispatch_bytes: fields[6],
                locator: fields[7],
                scan_recovery: fields[8],
                arenas: fields[9],
                citation: fields[10],
            };
            if rows.insert(level.as_str(), row).is_some() {
                return Err(format!("duplicate Appendix C level {name}"));
            }
        }
        if rows.is_empty() {
            return Err("Appendix C has no rows".into());
        }
        for row in rows.values() {
            for parent in &row.parents {
                if let PartParent::Level(parent) = parent {
                    if !rows.contains_key(parent.as_str()) {
                        return Err(format!(
                            "parent {} is absent from the table",
                            parent.as_str()
                        ));
                    }
                }
            }
        }
        Ok(Self { rows })
    }

    /// Look up an exact typed level; absent rows are errors.
    pub fn rule(&self, level: PartLevel) -> Result<&PartRule<'a>, String> {
        self.rows
            .get(level.as_str())
            .ok_or_else(|| format!("Appendix C row {} is absent", level.as_str()))
    }

    /// Number of qualified rows in this table.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether this table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}
