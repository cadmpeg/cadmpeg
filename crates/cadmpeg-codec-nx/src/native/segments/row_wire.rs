use serde::{Deserialize, Deserializer, Serializer};

pub(super) fn serialize<S: Serializer>(row: &usize, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("nx:segment-index:row#{row}"))
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let value = String::deserialize(deserializer)?;
    let row = value
        .strip_prefix("nx:segment-index:row#")
        .and_then(|ordinal| ordinal.parse::<usize>().ok())
        .ok_or_else(|| serde::de::Error::custom("row must name a segment-index ordinal"))?;
    if value != format!("nx:segment-index:row#{row}") {
        return Err(serde::de::Error::custom(
            "row must use a canonical segment-index ordinal",
        ));
    }
    Ok(row)
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
