use serde::{Deserialize, Serialize};

/// Pointed and signature offsets of one OM section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub(crate) struct OmLocation {
    source_offset: u64,
    section_offset: u64,
}

impl OmLocation {
    pub(crate) fn new(source_offset: u64, separator_byte_len: u32) -> Option<Self> {
        Some(Self {
            source_offset,
            section_offset: source_offset.checked_add(u64::from(separator_byte_len))?,
        })
    }

    pub(crate) fn source_offset(self) -> u64 {
        self.source_offset
    }

    pub(crate) fn section_offset(self) -> u64 {
        self.section_offset
    }

    pub(crate) fn separator_byte_len(self) -> u32 {
        (self.section_offset - self.source_offset) as u32
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    separator_byte_len: u32,
    source_offset: u64,
    section_offset: u64,
}

impl From<OmLocation> for Wire {
    fn from(value: OmLocation) -> Self {
        Self {
            separator_byte_len: value.separator_byte_len(),
            source_offset: value.source_offset(),
            section_offset: value.section_offset(),
        }
    }
}

impl TryFrom<Wire> for OmLocation {
    type Error = &'static str;

    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let location = Self::new(wire.source_offset, wire.separator_byte_len)
            .ok_or("source_offset + separator_byte_len overflows section_offset")?;
        if location.section_offset() != wire.section_offset {
            return Err("section_offset must equal source_offset + separator_byte_len");
        }
        Ok(location)
    }
}

#[cfg(test)]
mod tests {
    use super::OmLocation;

    #[test]
    fn section_location_checks_wire_relationship_and_overflow() {
        for separator_byte_len in [0, 4, u32::MAX] {
            let wire = serde_json::json!({
                "source_offset": 10, "separator_byte_len": separator_byte_len,
                "section_offset": 10 + u64::from(separator_byte_len)
            });
            let location: OmLocation = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(location).unwrap(), wire);
            let mut invalid = wire;
            invalid["section_offset"] = 0.into();
            assert!(serde_json::from_value::<OmLocation>(invalid)
                .unwrap_err()
                .to_string()
                .contains("section_offset"));
        }
        assert!(OmLocation::new(u64::MAX, 1).is_none());
        let wire = serde_json::json!({
            "source_offset": u64::MAX, "separator_byte_len": 1, "section_offset": 0
        });
        assert!(serde_json::from_value::<OmLocation>(wire).is_err());
    }
}
