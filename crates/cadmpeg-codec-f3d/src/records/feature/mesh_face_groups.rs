use crate::records::entity_header::SegmentTypeData;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::identity::Located;
use serde::{Deserialize, Serialize};

pub(crate) const SCOPE_TYPES: &[(&str, u32, DesignFeatureKind)] = &[
    (
        "CA2AE956-FD81-4EBD-A601-F93A0F335722",
        3,
        DesignFeatureKind::GenerateFaceGroups,
    ),
    (
        "8136D47E-3955-4682-8BD7-B353B7B45624",
        1,
        DesignFeatureKind::CombineFaceGroups,
    ),
];

pub(crate) fn registered_kind(info: &SegmentTypeData) -> Option<DesignFeatureKind> {
    SCOPE_TYPES
        .iter()
        .find(|(guid, version, _)| {
            info.type_guid.as_str().eq_ignore_ascii_case(guid)
                && info.version == *version
                && info.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case("90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF")
                })
                && info.module == "Fusion"
        })
        .map(|(_, _, kind)| kind.clone())
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignMeshFaceGroups {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_method_code"
    )]
    pub(crate) method_code: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_hint"
    )]
    pub(crate) group_hint: Option<Located<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_related_mesh"
    )]
    pub(crate) related_mesh: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_mesh"
    )]
    pub(crate) source_mesh: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_count"
    )]
    pub(crate) group_count: Option<Located<u32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) groups: Vec<Located<String>>,
}

cadmpeg_core::named_optional_field!(deserialize_method_code, Located<u32>, "method_code");
cadmpeg_core::named_optional_field!(deserialize_group_hint, Located<String>, "group_hint");
cadmpeg_core::named_optional_field!(deserialize_related_mesh, Located<u32>, "related_mesh");
cadmpeg_core::named_optional_field!(deserialize_source_mesh, Located<u32>, "source_mesh");
cadmpeg_core::named_optional_field!(deserialize_group_count, Located<u32>, "group_count");

rewrite_native_scalar!(DesignMeshFaceGroups);
