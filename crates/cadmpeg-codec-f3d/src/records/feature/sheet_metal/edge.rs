use super::DesignEdgeFlangeOperation;
use crate::records::identity::Located;
use cadmpeg_ir::scalar::PositiveReal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum DesignEdgeFlangeScope {
    #[default]
    Unresolved,
    Legacy(DesignEdgeFlangeOperation),
    ReferenceFramed(DesignReferenceEdgeFlange),
}

impl From<Option<DesignEdgeFlangeOperation>> for DesignEdgeFlangeScope {
    fn from(value: Option<DesignEdgeFlangeOperation>) -> Self {
        value.map_or(Self::Unresolved, Self::Legacy)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignReferenceEdgeFlange {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bend_position"
    )]
    pub(crate) bend_position: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_count"
    )]
    pub(crate) edge_count: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height_datum"
    )]
    pub(crate) height_datum: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radius_control"
    )]
    pub(crate) radius_control: Option<Located<PositiveReal>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle_owner"
    )]
    pub(crate) angle_owner: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height_owner"
    )]
    pub(crate) height_owner: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_settings"
    )]
    pub(crate) settings: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_aggregate_group"
    )]
    pub(crate) aggregate_group: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_result_count"
    )]
    pub(crate) result_count: Option<Located<u32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) edges: Vec<DesignReferenceFlangeEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) results: Vec<DesignReferenceFlangeResult>,
}
cadmpeg_core::named_optional_field!(deserialize_bend_position, Located<u32>, "bend_position");
cadmpeg_core::named_optional_field!(deserialize_edge_count, Located<u32>, "edge_count");
cadmpeg_core::named_optional_field!(deserialize_height_datum, Located<u32>, "height_datum");
cadmpeg_core::named_optional_field!(
    deserialize_radius_control,
    Located<PositiveReal>,
    "radius_control"
);
cadmpeg_core::named_optional_field!(deserialize_angle_owner, Located<u32>, "angle_owner");
cadmpeg_core::named_optional_field!(deserialize_height_owner, Located<u32>, "height_owner");
cadmpeg_core::named_optional_field!(deserialize_settings, Located<u32>, "settings");
cadmpeg_core::named_optional_field!(deserialize_aggregate_group, Located<u32>, "aggregate_group");
cadmpeg_core::named_optional_field!(deserialize_result_count, Located<u32>, "result_count");
rewrite_native_record!(DesignReferenceEdgeFlange, []; {bend_position, edge_count, height_datum, radius_control, angle_owner, height_owner, settings, aggregate_group, result_count, edges, results});

impl DesignReferenceEdgeFlange {
    pub(crate) fn valid_for_scope(
        &self,
        scope: &super::super::scope::DesignParameterScope,
    ) -> bool {
        let within = |offset: u64, width: u64| {
            offset >= scope.byte_offset()
                && offset
                    .checked_add(width)
                    .is_some_and(|end| end <= scope.reference_count_offset())
        };
        let member = |field: &Located<u32>| {
            scope
                .reference_members()
                .values()
                .any(|value| *value == field.value)
        };
        [
            self.bend_position.as_ref(),
            self.edge_count.as_ref(),
            self.height_datum.as_ref(),
            self.result_count.as_ref(),
        ]
        .into_iter()
        .flatten()
        .all(|field| within(field.offset, 4))
            && self
                .radius_control
                .as_ref()
                .is_none_or(|field| within(field.offset, 8))
            && self.edge_count.as_ref().is_none_or(|count| {
                usize::try_from(count.value).ok() == Some(self.edges.len()) && count.value != 0
            })
            && self
                .result_count
                .as_ref()
                .is_none_or(|count| usize::try_from(count.value).ok() == Some(self.results.len()))
            && [
                self.angle_owner.as_ref(),
                self.height_owner.as_ref(),
                self.settings.as_ref(),
                self.aggregate_group.as_ref(),
            ]
            .into_iter()
            .flatten()
            .all(|field| member(field) && within(field.offset, 1))
            && self.edges.iter().all(|edge| {
                [edge.wrapper.as_ref(), edge.group.as_ref()]
                    .into_iter()
                    .flatten()
                    .all(|field| member(field) && within(field.offset, 1))
                    && [
                        edge.first_owner.as_ref(),
                        edge.second_owner.as_ref(),
                        edge.first_boundary.as_ref(),
                        edge.second_boundary.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .all(member)
                    && edge
                        .wrapper_member_count
                        .as_ref()
                        .is_none_or(|count| count.value == 5)
            })
            && self.results.iter().enumerate().all(|(ordinal, result)| {
                result
                    .record
                    .as_ref()
                    .is_none_or(|field| within(field.offset, 1))
                    && result.trailer.as_ref().is_none_or(|field| {
                        within(field.offset, 4)
                            && field.value == u32::from(ordinal + 1 < self.results.len())
                    })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignReferenceFlangeEdge {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_wrapper"
    )]
    pub(crate) wrapper: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group"
    )]
    pub(crate) group: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_wrapper_member_count"
    )]
    pub(crate) wrapper_member_count: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_owner"
    )]
    pub(crate) first_owner: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_owner"
    )]
    pub(crate) second_owner: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_boundary"
    )]
    pub(crate) first_boundary: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_boundary"
    )]
    pub(crate) second_boundary: Option<Located<u32>>,
}
cadmpeg_core::named_optional_field!(deserialize_wrapper, Located<u32>, "wrapper");
cadmpeg_core::named_optional_field!(deserialize_group, Located<u32>, "group");
cadmpeg_core::named_optional_field!(
    deserialize_wrapper_member_count,
    Located<u32>,
    "wrapper_member_count"
);
cadmpeg_core::named_optional_field!(deserialize_first_owner, Located<u32>, "first_owner");
cadmpeg_core::named_optional_field!(deserialize_second_owner, Located<u32>, "second_owner");
cadmpeg_core::named_optional_field!(deserialize_first_boundary, Located<u32>, "first_boundary");
cadmpeg_core::named_optional_field!(deserialize_second_boundary, Located<u32>, "second_boundary");
rewrite_native_record!(DesignReferenceFlangeEdge, []; {wrapper, group, wrapper_member_count, first_owner, second_owner, first_boundary, second_boundary});

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignReferenceFlangeResult {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record"
    )]
    pub(crate) record: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_trailer"
    )]
    pub(crate) trailer: Option<Located<u32>>,
}
cadmpeg_core::named_optional_field!(deserialize_record, Located<u32>, "record");
cadmpeg_core::named_optional_field!(deserialize_trailer, Located<u32>, "trailer");
rewrite_native_record!(DesignReferenceFlangeResult, []; {record, trailer});

rewrite_native_enum!(DesignEdgeFlangeScope, []; {
    Unresolved,
    Legacy(field0),
    ReferenceFramed(field0),
});

#[cfg(test)]
mod tests;
