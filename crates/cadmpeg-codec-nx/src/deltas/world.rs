// SPDX-License-Identifier: Apache-2.0
//! WORLD layouts selected by the preceding effective schema, never by length.

use super::embedded_schema::{DataType, EmbeddedSchema};
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorldLayout {
    Legacy,
    Mesh,
    Lattice,
}

const PREFIX: [(&str, DataType, u16); 7] = [
    ("assembly", DataType::Pointer, 10),
    ("attribute", DataType::Pointer, 81),
    ("body", DataType::Pointer, 12),
    ("transform", DataType::Pointer, 100),
    ("surface", DataType::Pointer, 1006),
    ("curve", DataType::Pointer, 1008),
    ("point", DataType::Pointer, 29),
];
const MESH: [(&str, DataType, u16); 2] = [
    ("mesh", DataType::Pointer, 1006),
    ("polyline", DataType::Pointer, 1008),
];
const LATTICE: [(&str, DataType, u16); 1] = [("lattice", DataType::Pointer, 222)];
const SUFFIX: [(&str, DataType, u16); 8] = [
    ("alive", DataType::Logical, 0),
    ("attrib_def", DataType::Pointer, 80),
    ("attdef_list", DataType::Pointer, 74),
    ("highest_id", DataType::Integer, 0),
    ("current_id", DataType::Integer, 0),
    ("index_map_offset", DataType::Integer, 0),
    ("index_map", DataType::Pointer, 82),
    ("schema_embedding_map", DataType::Pointer, 82),
];
const MESH_TAIL: [(&str, DataType, u16); 1] = [("mesh_offset_data", DataType::Pointer, 206)];

impl WorldLayout {
    pub(super) fn from_schema(schema: &EmbeddedSchema) -> Option<Self> {
        [Self::Legacy, Self::Mesh, Self::Lattice]
            .into_iter()
            .find(|layout| {
                let mesh = !matches!(layout, Self::Legacy);
                schema.matches_fields(
                    101,
                    PREFIX
                        .into_iter()
                        .chain(MESH.into_iter().filter(|_| mesh))
                        .chain(
                            LATTICE
                                .into_iter()
                                .filter(|_| matches!(layout, Self::Lattice)),
                        )
                        .chain(SUFFIX)
                        .chain(MESH_TAIL.into_iter().filter(|_| mesh)),
                )
            })
    }

    /// Read one status-framed instance without allocating during candidate probes.
    pub(super) fn read(
        self,
        stream: &[u8],
        mut at: usize,
    ) -> Option<(u32, WorldReferences, usize)> {
        let xmt = u32::from(NonNullXmt::try_from(positive_xmt(stream, &mut at)?).ok()?);
        let leading = match self {
            Self::Legacy => 7,
            Self::Mesh => 9,
            Self::Lattice => 10,
        };
        let total = match self {
            Self::Legacy => 11,
            Self::Mesh => 14,
            Self::Lattice => 15,
        };
        let mut references = [0; 15];
        for reference in &mut references[..leading] {
            *reference = reference_at(stream, &mut at)?;
        }
        let mut view = View::over_retained(stream);
        view.seek(at)?;
        match view.u8()? {
            0 | 1 => {}
            _ => return None,
        }
        at = view.position();
        for reference in &mut references[leading..leading + 2] {
            *reference = reference_at(stream, &mut at)?;
        }
        view.seek(at)?;
        for _ in 0..3 {
            view.u32_be()?;
        }
        at = view.position();
        for reference in &mut references[leading + 2..total] {
            *reference = reference_at(stream, &mut at)?;
        }
        let references = match self {
            Self::Legacy => WorldReferences::Legacy(references[..11].try_into().ok()?),
            Self::Mesh => WorldReferences::Mesh(references[..14].try_into().ok()?),
            Self::Lattice => WorldReferences::Lattice(references),
        };
        Some((xmt, references, at))
    }
}

fn reference_at(stream: &[u8], at: &mut usize) -> Option<u32> {
    let target = positive_xmt(stream, at)?;
    match *stream.get(*at)? {
        0 | 1 => {}
        _ => return None,
    }
    *at = at.checked_add(1)?;
    Some(target)
}

fn positive_xmt(stream: &[u8], at: &mut usize) -> Option<u32> {
    let mut view = View::over_retained(stream);
    view.seek(*at)?;
    let target = super::embedded_schema::positive(&mut view)?.checked_add(1)?;
    *at = view.position();
    Some(target)
}

/// The pointer sequence has a cardinality fixed by the declared WORLD layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorldReferences {
    Legacy([u32; 11]),
    Mesh([u32; 14]),
    Lattice([u32; 15]),
}

impl WorldReferences {
    pub(super) fn values(&self) -> &[u32] {
        match self {
            Self::Legacy(values) => values,
            Self::Mesh(values) => values,
            Self::Lattice(values) => values,
        }
    }
}

impl TryFrom<Vec<u32>> for WorldReferences {
    type Error = &'static str;
    fn try_from(values: Vec<u32>) -> Result<Self, Self::Error> {
        if values.contains(&0) {
            return Err("WORLD references: zero target");
        }
        match values.len() {
            11 => Ok(Self::Legacy(
                values
                    .try_into()
                    .map_err(|_| "WORLD references: expected eleven")?,
            )),
            14 => Ok(Self::Mesh(
                values
                    .try_into()
                    .map_err(|_| "WORLD references: expected fourteen")?,
            )),
            15 => Ok(Self::Lattice(
                values
                    .try_into()
                    .map_err(|_| "WORLD references: expected fifteen")?,
            )),
            _ => Err("WORLD references: expected eleven, fourteen, or fifteen"),
        }
    }
}

#[cfg(test)]
mod tests;
