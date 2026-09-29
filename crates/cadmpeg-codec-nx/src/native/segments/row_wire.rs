use super::{SegmentIndexSlot, SegmentStreamLink};
use crate::parasolid::StreamKind;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct Wire {
    id: String,
    row: String,
    slot: SegmentIndexSlot,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    wrapper_byte_len: u32,
    source_offset: u64,
}

struct RowId(usize);

impl std::fmt::Display for RowId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "nx:segment-index:row#{}", self.0)
    }
}

impl Serialize for RowId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Serialize)]
struct WireRef<'a> {
    id: &'a str,
    row: RowId,
    slot: SegmentIndexSlot,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    wrapper_byte_len: u32,
    source_offset: u64,
}

impl Serialize for SegmentStreamLink {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireRef {
            id: &self.id,
            row: RowId(self.row),
            slot: self.slot,
            stream_ordinal: self.stream_ordinal,
            stream_kind: self.stream_kind,
            wrapper_byte_len: self.wrapper_byte_len,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
std::thread_local! {
    static LINK_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<SegmentStreamLink> for Wire {
    fn from(link: SegmentStreamLink) -> Self {
        LINK_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
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
    use super::{Wire, LINK_INTO_WIRE_COUNT};
    use crate::native::segments::{SegmentStreamLink, STREAM_LINK_CLONE_COUNT};

    #[test]
    fn row_identity_round_trips_and_rejects_noncanonical_strings() {
        let wire = serde_json::json!({
            "id": "link", "row": "nx:segment-index:row#2", "slot": "type_code",
            "stream_ordinal": 0, "stream_kind": "partition", "wrapper_byte_len": 8,
            "source_offset": 0
        });
        let link: SegmentStreamLink = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(link.row, 2);
        assert_eq!(
            serde_json::to_vec(&link).unwrap(),
            serde_json::to_vec(&Wire::from(link.clone())).unwrap()
        );
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

    #[test]
    fn segment_stream_link_retained_limit_refuses_before_clone_or_row_format() {
        let wire = serde_json::json!({
            "id": "nx:segment-stream:link#1", "row": "nx:segment-index:row#2",
            "slot": "type_code", "stream_ordinal": 0, "stream_kind": "partition",
            "wrapper_byte_len": 8, "source_offset": 0
        });
        let link: SegmentStreamLink = serde_json::from_value(wire.clone()).unwrap();
        STREAM_LINK_CLONE_COUNT.with(|count| count.set(0));
        LINK_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(&link, wire);
        STREAM_LINK_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        LINK_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
