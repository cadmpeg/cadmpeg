// SPDX-License-Identifier: Apache-2.0
//! Public modern WORLD fields, ending before the next deltas event.

use crate::framing::read_and_advance;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ReferenceState {
    target: NonZeroU32,
    status: bool,
}

impl ReferenceState {
    fn read(stream: &[u8], at: &mut usize) -> Option<Self> {
        let target = NonZeroU32::new(read_and_advance(stream, at)?)?;
        let status = match *stream.get(*at)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        *at = at.checked_add(1)?;
        Some(Self { target, status })
    }
}

/// The nineteen schema-declared fields and the WORLD node identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorldState {
    xmt: NonNullXmt,
    assembly: ReferenceState,
    attribute: ReferenceState,
    body: ReferenceState,
    transform: ReferenceState,
    surface: ReferenceState,
    curve: ReferenceState,
    point: ReferenceState,
    mesh: ReferenceState,
    polyline: ReferenceState,
    lattice: ReferenceState,
    alive: bool,
    attrib_def: ReferenceState,
    attdef_list: ReferenceState,
    highest_id: u32,
    current_id: u32,
    index_map_offset: u32,
    index_map: ReferenceState,
    schema_embedding_map: ReferenceState,
    mesh_offset_data: ReferenceState,
}

impl WorldState {
    pub(super) fn xmt(self) -> u32 {
        self.xmt.into()
    }

    pub(super) fn reference_targets(self) -> [u32; 15] {
        [
            self.assembly,
            self.attribute,
            self.body,
            self.transform,
            self.surface,
            self.curve,
            self.point,
            self.mesh,
            self.polyline,
            self.lattice,
            self.attrib_def,
            self.attdef_list,
            self.index_map,
            self.schema_embedding_map,
            self.mesh_offset_data,
        ]
        .map(|reference| reference.target.get())
    }

    pub(super) fn read(stream: &[u8], mut at: usize) -> Option<(Self, usize)> {
        let xmt = NonNullXmt::try_from(read_and_advance(stream, &mut at)?).ok()?;
        let assembly = ReferenceState::read(stream, &mut at)?;
        let attribute = ReferenceState::read(stream, &mut at)?;
        let body = ReferenceState::read(stream, &mut at)?;
        let transform = ReferenceState::read(stream, &mut at)?;
        let surface = ReferenceState::read(stream, &mut at)?;
        let curve = ReferenceState::read(stream, &mut at)?;
        let point = ReferenceState::read(stream, &mut at)?;
        let mesh = ReferenceState::read(stream, &mut at)?;
        let polyline = ReferenceState::read(stream, &mut at)?;
        let lattice = ReferenceState::read(stream, &mut at)?;
        let mut view = View::over_retained(stream);
        view.seek(at)?;
        let alive = match view.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        at = view.position();
        let attrib_def = ReferenceState::read(stream, &mut at)?;
        let attdef_list = ReferenceState::read(stream, &mut at)?;
        view.seek(at)?;
        let highest_id = view.u32_be()?;
        let current_id = view.u32_be()?;
        let index_map_offset = view.u32_be()?;
        at = view.position();
        let index_map = ReferenceState::read(stream, &mut at)?;
        let schema_embedding_map = ReferenceState::read(stream, &mut at)?;
        let mesh_offset_data = ReferenceState::read(stream, &mut at)?;
        Some((
            Self {
                xmt,
                assembly,
                attribute,
                body,
                transform,
                surface,
                curve,
                point,
                mesh,
                polyline,
                lattice,
                alive,
                attrib_def,
                attdef_list,
                highest_id,
                current_id,
                index_map_offset,
                index_map,
                schema_embedding_map,
                mesh_offset_data,
            },
            at,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::WorldState;

    fn source() -> Vec<u8> {
        let mut bytes = 2_u16.to_be_bytes().to_vec();
        for _ in 0..10 {
            bytes.extend_from_slice(&[0, 1, 1]);
        }
        bytes.push(0);
        for _ in 0..2 {
            bytes.extend_from_slice(&[0, 1, 0]);
        }
        for word in [u32::MAX, 0, 0x0001_0001] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        for _ in 0..3 {
            bytes.extend_from_slice(&[0, 1, 1]);
        }
        bytes
    }

    #[test]
    fn world_fields_preserve_status_and_full_scalars_without_reading_the_successor() {
        let bytes = source();
        let (state, end) = WorldState::read(&bytes, 0).unwrap();
        assert_eq!(end, 60);
        assert_eq!(state.highest_id, u32::MAX);
        assert_eq!(state.current_id, 0);
        assert_eq!(state.index_map_offset, 0x0001_0001);
        assert!(!state.alive);
        assert_eq!(state.assembly.target.get(), 1);
        assert!(state.assembly.status);
        assert!(!state.attrib_def.status);
        let mut followed = bytes.clone();
        followed.extend_from_slice(&[0, 1, 0, 1, 0, 4]);
        assert_eq!(WorldState::read(&followed, 0), Some((state, end)));
        for len in 0..bytes.len() {
            assert!(WorldState::read(&bytes[..len], 0).is_none());
        }
        for offset in [4, 32] {
            let mut invalid = bytes.clone();
            invalid[offset] = 2;
            assert!(WorldState::read(&invalid, 0).is_none());
        }
    }

    #[test]
    fn world_wire_retains_named_fields_and_rejects_zero_targets_and_null_identity() {
        let (state, _) = WorldState::read(&source(), 0).unwrap();
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["index_map_offset"], 0x0001_0001);
        assert_eq!(
            value["attrib_def"],
            serde_json::json!({"target": 1, "status": false})
        );
        assert_eq!(
            serde_json::from_value::<WorldState>(value.clone()).unwrap(),
            state
        );
        for identity in [0, 1] {
            let mut invalid = value.clone();
            invalid["xmt"] = identity.into();
            assert!(serde_json::from_value::<WorldState>(invalid).is_err());
        }
        for field in [
            "assembly",
            "attribute",
            "body",
            "transform",
            "surface",
            "curve",
            "point",
            "mesh",
            "polyline",
            "lattice",
            "attrib_def",
            "attdef_list",
            "index_map",
            "schema_embedding_map",
            "mesh_offset_data",
        ] {
            let mut invalid = value.clone();
            invalid[field]["target"] = 0.into();
            assert!(serde_json::from_value::<WorldState>(invalid).is_err());
        }
    }
}
