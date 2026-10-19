use super::dimensions::{DesignDimensionLocusPair, DesignDimensionLocusPairWire};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wire {
    id: String,
    companion_record_index: u32,
    governing_companion_record_index: u32,
    class_tag: String,
    record_index: u32,
    null_role: u32,
    geometry_record_index: u32,
    geometry_role: u32,
    paired_class_tag: String,
}

#[cfg(test)]
thread_local! {
    pub(crate) static OWNED_WIRE_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Serialize)]
pub(crate) struct BorrowedWire<'a> {
    id: &'a str,
    companion_record_index: u32,
    governing_companion_record_index: u32,
    class_tag: &'a str,
    record_index: u32,
    null_role: u32,
    geometry_record_index: u32,
    geometry_role: u32,
    paired_class_tag: &'a str,
}

impl<'a> From<&'a DesignDimensionLocusPair> for BorrowedWire<'a> {
    fn from(pair: &'a DesignDimensionLocusPair) -> Self {
        let [first, second] = pair.loci();
        Self {
            id: &pair.id,
            companion_record_index: pair.companion_record_index,
            governing_companion_record_index: pair.governing_companion_record_index,
            class_tag: pair.class_tag.as_str(),
            record_index: pair.record_index,
            null_role: first.role,
            geometry_record_index: second
                .geometry_record_index
                .map_or(0, std::num::NonZeroU32::get),
            geometry_role: second.role,
            paired_class_tag: pair.paired_class_tag.as_str(),
        }
    }
}

impl From<&DesignDimensionLocusPair> for Wire {
    fn from(pair: &DesignDimensionLocusPair) -> Self {
        #[cfg(test)]
        OWNED_WIRE_CONVERSIONS.with(|count| count.set(count.get() + 1));
        let [first, second] = pair.loci();
        Self {
            id: pair.id.clone(),
            companion_record_index: pair.companion_record_index,
            governing_companion_record_index: pair.governing_companion_record_index,
            class_tag: pair.class_tag.as_str().to_owned(),
            record_index: pair.record_index,
            null_role: first.role,
            geometry_record_index: second
                .geometry_record_index
                .map_or(0, std::num::NonZeroU32::get),
            geometry_role: second.role,
            paired_class_tag: pair.paired_class_tag.as_str().to_owned(),
        }
    }
}

#[derive(Deserialize)]
#[serde(try_from = "Wire")]
pub(crate) struct Entry(pub(crate) DesignDimensionLocusPair);

impl TryFrom<Wire> for Entry {
    type Error = String;

    /// A null-locus frame with `geometry_record_index == 0` is not decoder-producible and is rejected deliberately.
    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        if wire.geometry_record_index == 0 {
            return Err("geometry_record_index must name an indexed sketch-geometry record".into());
        }
        DesignDimensionLocusPair::try_from(DesignDimensionLocusPairWire {
            id: wire.id,
            companion_record_index: wire.companion_record_index,
            governing_companion_record_index: wire.governing_companion_record_index,
            class_tag: wire.class_tag,
            record_index: wire.record_index,
            opaque_index: None,
            first_geometry_record_index: 0,
            first_role: wire.null_role,
            second_geometry_record_index: wire.geometry_record_index,
            second_role: wire.geometry_role,
            paired_class_tag: wire.paired_class_tag,
        })
        .map(Self)
    }
}
