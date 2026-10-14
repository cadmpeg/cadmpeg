// SPDX-License-Identifier: Apache-2.0
//! Exact FSET counted and empty lanes with unresolved index targets.

use super::{format_feature_history_id, visit_feature_history_operation_records};
use crate::container::Container;
use crate::om::compact::{
    CompactIndexAtom, CountedIndexMembers, LocatedCompactIndex, RawCompactIndex,
};
use crate::om::fset_indices::{FsetIndexLane, FsetIndexValues};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "LaneWire")]
pub(in crate::native) struct FeatureFsetIndexLane {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    lane: FsetIndexLane,
}

#[derive(Deserialize)]
struct LaneWire {
    id: String,
    operation_label: String,
    parameter_class_index: u8,
    source_offset: u64,
    layout: LayoutWire,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LayoutWire {
    Empty {},
    Counted {
        declared_count: u8,
        members: Vec<MemberWire>,
    },
}

#[derive(Deserialize)]
struct MemberWire {
    value: u32,
    raw_index: NativeBytes<Vec<u8>>,
    source_offset: u64,
}

impl TryFrom<LaneWire> for FeatureFsetIndexLane {
    type Error = String;

    fn try_from(wire: LaneWire) -> Result<Self, Self::Error> {
        let values = match wire.layout {
            LayoutWire::Empty {} => FsetIndexValues::Empty,
            LayoutWire::Counted {
                declared_count,
                members,
            } => {
                let members = members
                    .into_iter()
                    .map(|member| {
                        Ok(LocatedCompactIndex {
                            atom: CompactIndexAtom::from_wire(member.value, &member.raw_index)
                                .map_err(|error| format!("members.raw_index: {error}"))?,
                            offset: member.source_offset,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let members = CountedIndexMembers::<_, 1>::new(members).map_err(str::to_owned)?;
                if declared_count != members.declared_count() {
                    return Err("layout.declared_count: requires members.len() + 1".to_owned());
                }
                FsetIndexValues::Counted(members)
            }
        };
        let lane = FsetIndexLane::new(wire.parameter_class_index, wire.source_offset, values)
            .map_err(str::to_owned)?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            lane,
        })
    }
}

#[derive(Serialize)]
struct LaneRef<'a> {
    id: &'a str,
    operation_label: &'a str,
    parameter_class_index: u8,
    source_offset: u64,
    layout: LayoutRef<'a>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LayoutRef<'a> {
    Empty,
    Counted {
        declared_count: u8,
        members: MembersRef<'a>,
    },
}

#[derive(Serialize)]
struct MemberRef {
    value: u32,
    raw_index: RawCompactIndex,
    source_offset: u64,
}

struct MembersRef<'a>(&'a [LocatedCompactIndex<u64>]);

impl Serialize for MembersRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for member in self.0 {
            sequence.serialize_element(&MemberRef {
                value: member.atom.value(),
                raw_index: RawCompactIndex(member.atom),
                source_offset: member.offset,
            })?;
        }
        sequence.end()
    }
}

impl Serialize for FeatureFsetIndexLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let layout = match self.lane.values() {
            FsetIndexValues::Empty => LayoutRef::Empty,
            FsetIndexValues::Counted(members) => LayoutRef::Counted {
                declared_count: members.declared_count(),
                members: MembersRef(members.as_slice()),
            },
        };
        LaneRef {
            id: &self.id,
            operation_label: &self.operation_label,
            parameter_class_index: self.lane.parameter_class_index(),
            source_offset: self.lane.source_offset(),
            layout,
        }
        .serialize(serializer)
    }
}

pub(in crate::native) fn feature_fset_index_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureFsetIndexLane>, CodecError> {
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let Some(lane) = FsetIndexLane::read(ctx, record.payload_view(), entry_offset)?
                else {
                    return Ok(());
                };
                let id = format_feature_history_id(
                    ctx,
                    "fset-index-lane",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.reserve_vec(&mut lanes, 1, "NX FSET index lanes")?;
                lanes.push(FeatureFsetIndexLane {
                    id,
                    operation_label,
                    lane,
                });
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(lanes)
}

#[cfg(test)]
mod tests {
    use super::FeatureFsetIndexLane;

    const COUNTED: &str = r#"{"id":"nx:feature:fset-index-lane#0-0000000001","operation_label":"nx:feature:operation-label#0-0000000001","parameter_class_index":49,"source_offset":100,"layout":{"kind":"counted","declared_count":3,"members":[{"value":1303,"raw_index":"8517","source_offset":123},{"value":94,"raw_index":"5e","source_offset":125}]}}"#;
    const EMPTY: &str = r#"{"id":"nx:feature:fset-index-lane#0-0000000001","operation_label":"nx:feature:operation-label#0-0000000001","parameter_class_index":49,"source_offset":100,"layout":{"kind":"empty"}}"#;

    #[test]
    fn fset_lane_wire_roundtrips_and_obeys_retained_serialization_limits() {
        for json in [COUNTED, EMPTY] {
            let record: FeatureFsetIndexLane = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            cadmpeg_test_support::native_serialization::assert_native_limit(
                &record,
                serde_json::from_str::<serde_json::Value>(json).unwrap(),
            );
        }
    }

    #[test]
    fn fset_lane_wire_rejects_value_count_and_source_position_changes() {
        for (path, value) in [
            ("/parameter_class_index", serde_json::json!(0)),
            ("/source_offset", serde_json::json!(u64::MAX)),
            ("/layout/declared_count", serde_json::json!(4)),
            ("/layout/members/0/value", serde_json::json!(133)),
            ("/layout/members/0/raw_index", serde_json::json!("8517ff")),
            ("/layout/members/1/source_offset", serde_json::json!(124)),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(COUNTED).unwrap();
            *wire.pointer_mut(path).unwrap() = value;
            assert!(serde_json::from_value::<FeatureFsetIndexLane>(wire).is_err());
        }
        let mut wire: serde_json::Value = serde_json::from_str(EMPTY).unwrap();
        wire["layout"]["declared_count"] = serde_json::json!(1);
        assert!(serde_json::from_value::<FeatureFsetIndexLane>(wire).is_err());
    }
}
