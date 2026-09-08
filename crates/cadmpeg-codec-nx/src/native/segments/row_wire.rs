use super::{SegmentIndexSlot, SegmentStreamLink};
use crate::parasolid::StreamKind;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Wire {
    id: String,
    row: String,
    slot: SegmentIndexSlot,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    wrapper_byte_len: u32,
    source_offset: u64,
}

impl From<SegmentStreamLink> for Wire {
    fn from(link: SegmentStreamLink) -> Self {
        Self {
            id: link.id,
            row: format!("nx:segment-index:row#{}", link.row),
            slot: link.slot,
            stream_ordinal: link.stream_ordinal,
            stream_kind: link.stream_kind,
            wrapper_byte_len: link.wrapper_byte_len,
            source_offset: link.source_offset,
        }
    }
}

impl TryFrom<Wire> for SegmentStreamLink {
    type Error = &'static str;

    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let row = wire
            .row
            .strip_prefix("nx:segment-index:row#")
            .and_then(|ordinal| ordinal.parse::<usize>().ok())
            .ok_or("row must name a segment-index ordinal")?;
        if wire.row != format!("nx:segment-index:row#{row}") {
            return Err("row must use a canonical segment-index ordinal");
        }
        Ok(Self {
            id: wire.id,
            row,
            slot: wire.slot,
            stream_ordinal: wire.stream_ordinal,
            stream_kind: wire.stream_kind,
            wrapper_byte_len: wire.wrapper_byte_len,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::native::segments::SegmentStreamLink;

    #[test]
    fn row_identity_round_trips_and_rejects_noncanonical_strings() {
        let wire = serde_json::json!({
            "id": "link", "row": "nx:segment-index:row#2", "slot": "type_code",
            "stream_ordinal": 0, "stream_kind": "partition", "wrapper_byte_len": 8,
            "source_offset": 0
        });
        let link: SegmentStreamLink = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(link.row, 2);
        assert_eq!(serde_json::to_value(link).unwrap(), wire);
        for row in [
            "",
            "other#2",
            "nx:segment-index:row#02",
            "nx:segment-index:row#-1",
        ] {
            let mut invalid = wire.clone();
            invalid["row"] = row.into();
            assert!(serde_json::from_value::<SegmentStreamLink>(invalid)
                .unwrap_err()
                .to_string()
                .contains("row"));
        }
    }
}
