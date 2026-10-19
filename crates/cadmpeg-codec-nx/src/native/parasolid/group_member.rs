// SPDX-License-Identifier: Apache-2.0
//! GROUP member families with their required node identities.

use super::ParasolidGroupMember;
use crate::framing::node_kind::NodeKind;
use crate::records::group::{GroupSelector, MembershipKind};
use crate::topology::Graph;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::native) enum GroupNodeFamily {
    Instance,
    Face,
    Loop,
    Edge,
    Vertex,
    Region,
    Point,
    Line,
    Circle,
    Ellipse,
    Intersection,
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BlendSurface,
    BlendBound,
    OffsetSurface,
    SweptSurface,
    SpunSurface,
    PeSurface,
    BSurface,
    PeCurve,
    TrimmedCurve,
    BCurve,
    SpCurve,
}
impl GroupNodeFamily {
    fn graph_kind(self) -> Option<NodeKind> {
        match self {
            Self::Instance => None,
            Self::Face => Some(NodeKind::Face),
            Self::Loop => Some(NodeKind::Loop),
            Self::Edge => Some(NodeKind::Edge),
            Self::Vertex => Some(NodeKind::Vertex),
            Self::Region => Some(NodeKind::Region),
            Self::Point => Some(NodeKind::Point),
            Self::Line => Some(NodeKind::Line),
            Self::Circle => Some(NodeKind::Circle),
            Self::Ellipse => Some(NodeKind::Ellipse),
            Self::Intersection => Some(NodeKind::Intersection),
            Self::Plane => Some(NodeKind::Plane),
            Self::Cylinder => Some(NodeKind::Cylinder),
            Self::Cone => Some(NodeKind::Cone),
            Self::Sphere => Some(NodeKind::Sphere),
            Self::Torus => Some(NodeKind::Torus),
            Self::BlendSurface => Some(NodeKind::BlendSurface),
            Self::BlendBound => None,
            Self::OffsetSurface => Some(NodeKind::OffsetSurface),
            Self::SweptSurface => None,
            Self::SpunSurface => None,
            Self::PeSurface => None,
            Self::BSurface => Some(NodeKind::BSurface),
            Self::PeCurve => None,
            Self::TrimmedCurve => Some(NodeKind::TrimmedCurve),
            Self::BCurve => Some(NodeKind::BCurve),
            Self::SpCurve => Some(NodeKind::SpCurve),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Instance => "INSTANCE",
            Self::Face => "FACE",
            Self::Loop => "LOOP",
            Self::Edge => "EDGE",
            Self::Vertex => "VERTEX",
            Self::Region => "REGION",
            Self::Point => "POINT",
            Self::Line => "LINE",
            Self::Circle => "CIRCLE",
            Self::Ellipse => "ELLIPSE",
            Self::Intersection => "INTERSECTION",
            Self::Plane => "PLANE",
            Self::Cylinder => "CYLINDER",
            Self::Cone => "CONE",
            Self::Sphere => "SPHERE",
            Self::Torus => "TORUS",
            Self::BlendSurface => "BLEND_SURF",
            Self::BlendBound => "BLEND_BOUND",
            Self::OffsetSurface => "OFFSET_SURF",
            Self::SweptSurface => "SWEPT_SURF",
            Self::SpunSurface => "SPUN_SURF",
            Self::PeSurface => "PE_SURF",
            Self::BSurface => "B_SURFACE",
            Self::PeCurve => "PE_CURVE",
            Self::TrimmedCurve => "TRIMMED_CURVE",
            Self::BCurve => "B_CURVE",
            Self::SpCurve => "SP_CURVE",
        }
    }
    fn selector(self) -> GroupSelector {
        match self {
            Self::Instance => GroupSelector::Instance,
            Self::Face => GroupSelector::Face,
            Self::Loop => GroupSelector::Loop,
            Self::Edge => GroupSelector::Edge,
            Self::Vertex => GroupSelector::Vertex,
            Self::Region => GroupSelector::Region,
            Self::Point => GroupSelector::Point,
            Self::Line => GroupSelector::Curve,
            Self::Circle => GroupSelector::Curve,
            Self::Ellipse => GroupSelector::Curve,
            Self::Intersection => GroupSelector::Curve,
            Self::Plane => GroupSelector::Surface,
            Self::Cylinder => GroupSelector::Surface,
            Self::Cone => GroupSelector::Surface,
            Self::Sphere => GroupSelector::Surface,
            Self::Torus => GroupSelector::Surface,
            Self::BlendSurface => GroupSelector::Surface,
            Self::BlendBound => GroupSelector::Surface,
            Self::OffsetSurface => GroupSelector::Surface,
            Self::SweptSurface => GroupSelector::Surface,
            Self::SpunSurface => GroupSelector::Surface,
            Self::PeSurface => GroupSelector::Surface,
            Self::BSurface => GroupSelector::Surface,
            Self::PeCurve => GroupSelector::Curve,
            Self::TrimmedCurve => GroupSelector::Curve,
            Self::BCurve => GroupSelector::Curve,
            Self::SpCurve => GroupSelector::Curve,
        }
    }
}
impl TryFrom<u16> for GroupNodeFamily {
    type Error = ();
    fn try_from(kind: u16) -> Result<Self, Self::Error> {
        Ok(match kind {
            11 => Self::Instance,
            14 => Self::Face,
            15 => Self::Loop,
            16 => Self::Edge,
            18 => Self::Vertex,
            19 => Self::Region,
            29 => Self::Point,
            30 => Self::Line,
            31 => Self::Circle,
            32 => Self::Ellipse,
            38 => Self::Intersection,
            50 => Self::Plane,
            51 => Self::Cylinder,
            52 => Self::Cone,
            53 => Self::Sphere,
            54 => Self::Torus,
            56 => Self::BlendSurface,
            59 => Self::BlendBound,
            60 => Self::OffsetSurface,
            67 => Self::SweptSurface,
            68 => Self::SpunSurface,
            120 => Self::PeSurface,
            124 => Self::BSurface,
            130 => Self::PeCurve,
            133 => Self::TrimmedCurve,
            134 => Self::BCurve,
            137 => Self::SpCurve,
            _ => return Err(()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::native) struct GroupMemberTarget {
    pub(in crate::native) family: GroupNodeFamily,
    pub(in crate::native) node_id: u32,
    pub(in crate::native) current_xmt: Option<u32>,
}
impl GroupMemberTarget {
    pub(super) fn from_membership(record: MembershipKind) -> Option<Self> {
        let MembershipKind::Node { kind, node_id } = record else {
            return None;
        };
        Some(Self {
            family: GroupNodeFamily::try_from(kind).ok()?,
            node_id,
            current_xmt: None,
        })
    }

    pub(super) fn matches_selector(self, selector: GroupSelector) -> bool {
        selector == GroupSelector::Mixed || selector == self.family.selector()
    }

    pub(super) fn resolve(self, graph: &Graph, member_xmt: u32) -> Self {
        let current_xmt = self.family.graph_kind().and_then(|kind| {
            graph
                .get(kind, member_xmt)
                .filter(|node| node.node_id() == Some(self.node_id))
                .map(crate::topology::Node::xmt)
                .or_else(|| graph.unique_xmt_by_node_id(kind, self.node_id))
        });
        Self {
            current_xmt,
            ..self
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct MemberWire {
    id: String,
    primary_stream_ordinal: u32,
    group_xmt: u32,
    group_node_id: u32,
    ordinal: u32,
    list_record_xmt: u32,
    member_xmt: u32,
    member_family: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_member_node_id"
    )]
    member_node_id: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_current_member_xmt"
    )]
    current_member_xmt: Option<u32>,
}

// The family and the node identity are stated together, so the refusal names
// which half the document left unstated.
cadmpeg_core::named_optional_field!(deserialize_member_node_id, u32, "member_node_id");

#[derive(Serialize)]
struct MemberRef<'a> {
    id: &'a str,
    primary_stream_ordinal: u32,
    group_xmt: u32,
    group_node_id: u32,
    ordinal: u32,
    list_record_xmt: u32,
    member_xmt: u32,
    member_family: &'static str,
    member_node_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_member_xmt: Option<u32>,
}

impl Serialize for ParasolidGroupMember {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        MemberRef {
            id: &self.id,
            primary_stream_ordinal: self.primary_stream_ordinal,
            group_xmt: self.group_xmt,
            group_node_id: self.group_node_id,
            ordinal: self.ordinal,
            list_record_xmt: self.list_record_xmt,
            member_xmt: self.member_xmt,
            member_family: self.target.family.name(),
            member_node_id: self.target.node_id,
            current_member_xmt: self.target.current_xmt,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<ParasolidGroupMember> for MemberWire {
    fn from(value: ParasolidGroupMember) -> Self {
        Self {
            id: value.id,
            primary_stream_ordinal: value.primary_stream_ordinal,
            group_xmt: value.group_xmt,
            group_node_id: value.group_node_id,
            ordinal: value.ordinal,
            list_record_xmt: value.list_record_xmt,
            member_xmt: value.member_xmt,
            member_family: value.target.family.name().into(),
            member_node_id: Some(value.target.node_id),
            current_member_xmt: value.target.current_xmt,
        }
    }
}
impl TryFrom<MemberWire> for ParasolidGroupMember {
    type Error = &'static str;
    fn try_from(wire: MemberWire) -> Result<Self, Self::Error> {
        let family = match wire.member_family.as_str() {
            "INSTANCE" => GroupNodeFamily::Instance,
            "FACE" => GroupNodeFamily::Face,
            "LOOP" => GroupNodeFamily::Loop,
            "EDGE" => GroupNodeFamily::Edge,
            "VERTEX" => GroupNodeFamily::Vertex,
            "REGION" => GroupNodeFamily::Region,
            "POINT" => GroupNodeFamily::Point,
            "LINE" => GroupNodeFamily::Line,
            "CIRCLE" => GroupNodeFamily::Circle,
            "ELLIPSE" => GroupNodeFamily::Ellipse,
            "INTERSECTION" => GroupNodeFamily::Intersection,
            "PLANE" => GroupNodeFamily::Plane,
            "CYLINDER" => GroupNodeFamily::Cylinder,
            "CONE" => GroupNodeFamily::Cone,
            "SPHERE" => GroupNodeFamily::Sphere,
            "TORUS" => GroupNodeFamily::Torus,
            "BLEND_SURF" => GroupNodeFamily::BlendSurface,
            "BLEND_BOUND" => GroupNodeFamily::BlendBound,
            "OFFSET_SURF" => GroupNodeFamily::OffsetSurface,
            "SWEPT_SURF" => GroupNodeFamily::SweptSurface,
            "SPUN_SURF" => GroupNodeFamily::SpunSurface,
            "PE_SURF" => GroupNodeFamily::PeSurface,
            "B_SURFACE" => GroupNodeFamily::BSurface,
            "PE_CURVE" => GroupNodeFamily::PeCurve,
            "TRIMMED_CURVE" => GroupNodeFamily::TrimmedCurve,
            "B_CURVE" => GroupNodeFamily::BCurve,
            "SP_CURVE" => GroupNodeFamily::SpCurve,
            _ => return Err("member_family: unsupported GROUP member family"),
        };
        let target = GroupMemberTarget {
            family,
            node_id: wire
                .member_node_id
                .ok_or("member_node_id: required by member_family")?,
            current_xmt: wire.current_member_xmt,
        };
        Ok(Self {
            id: wire.id,
            primary_stream_ordinal: wire.primary_stream_ordinal,
            group_xmt: wire.group_xmt,
            group_node_id: wire.group_node_id,
            ordinal: wire.ordinal,
            list_record_xmt: wire.list_record_xmt,
            member_xmt: wire.member_xmt,
            target,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ParasolidGroupMember, GROUP_MEMBER_CLONE_COUNT};
    use super::MemberWire;

    #[test]
    fn group_member_wire_covers_member_entities_and_rejects_nonmembers() {
        use super::GroupMemberTarget;
        use crate::records::group::{GroupSelector, MembershipKind};
        for (kind, name, selector) in [
            (11, "INSTANCE", 1),
            (14, "FACE", 2),
            (15, "LOOP", 3),
            (16, "EDGE", 4),
            (18, "VERTEX", 5),
            (19, "REGION", 10),
            (29, "POINT", 8),
            (30, "LINE", 7),
            (31, "CIRCLE", 7),
            (32, "ELLIPSE", 7),
            (38, "INTERSECTION", 7),
            (50, "PLANE", 6),
            (51, "CYLINDER", 6),
            (52, "CONE", 6),
            (53, "SPHERE", 6),
            (54, "TORUS", 6),
            (56, "BLEND_SURF", 6),
            (59, "BLEND_BOUND", 6),
            (60, "OFFSET_SURF", 6),
            (67, "SWEPT_SURF", 6),
            (68, "SPUN_SURF", 6),
            (120, "PE_SURF", 6),
            (124, "B_SURFACE", 6),
            (130, "PE_CURVE", 7),
            (133, "TRIMMED_CURVE", 7),
            (134, "B_CURVE", 7),
            (137, "SP_CURVE", 7),
        ] {
            let target =
                GroupMemberTarget::from_membership(MembershipKind::Node { kind, node_id: 50 })
                    .unwrap();
            assert!(target.matches_selector(GroupSelector::try_from(selector).unwrap()));
            assert!(target.matches_selector(GroupSelector::Mixed));
            assert_eq!(
                target.matches_selector(GroupSelector::Instance),
                selector == 1
            );
            let json = format!(
                r#"{{"id":"member","primary_stream_ordinal":4,"group_xmt":10,"group_node_id":7,"ordinal":0,"list_record_xmt":20,"member_xmt":30,"member_family":"{name}","member_node_id":50}}"#
            );
            let member: ParasolidGroupMember = serde_json::from_str(&json).unwrap();
            assert_eq!(member.target, target);
            assert_eq!(serde_json::to_string(&member).unwrap(), json);
        }
        for kind in [10, 12, 13, 17, 100] {
            assert!(
                GroupMemberTarget::from_membership(MembershipKind::Node { kind, node_id: 50 })
                    .is_none()
            );
        }
        for name in ["ASSEMBLY", "BODY", "SHELL", "FIN", "TRANSFORM"] {
            let json = format!(
                r#"{{"id":"member","primary_stream_ordinal":4,"group_xmt":10,"group_node_id":7,"ordinal":0,"list_record_xmt":20,"member_xmt":30,"member_family":"{name}","member_node_id":50}}"#
            );
            assert!(serde_json::from_str::<ParasolidGroupMember>(&json)
                .unwrap_err()
                .to_string()
                .contains("member_family"));
        }
    }

    #[test]
    fn family_owns_the_node_identity_on_the_wire() {
        for fields in [
            r#""member_family":"INSTANCE","member_node_id":50"#,
            r#""member_family":"FACE","member_node_id":50,"current_member_xmt":100"#,
        ] {
            let json = format!(
                r#"{{"id":"member","primary_stream_ordinal":4,"group_xmt":10,"group_node_id":7,"ordinal":0,"list_record_xmt":20,"member_xmt":30,{fields}}}"#
            );
            let member: ParasolidGroupMember = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&member).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&member).unwrap(),
                serde_json::to_vec(&MemberWire::from(member.clone())).unwrap()
            );
            let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            wire["member_node_id"] = serde_json::Value::Null;
            assert!(serde_json::from_value::<ParasolidGroupMember>(wire)
                .unwrap_err()
                .to_string()
                .contains("member_node_id"));
        }
    }

    #[test]
    fn group_member_retained_limit_refuses_before_clone() {
        for fields in [
            r#""member_family":"INSTANCE","member_node_id":50"#,
            r#""member_family":"FACE","member_node_id":50,"current_member_xmt":100"#,
        ] {
            let json = format!(
                r#"{{"id":"nx:s4:group-member#10-20","primary_stream_ordinal":4,"group_xmt":10,"group_node_id":7,"ordinal":0,"list_record_xmt":20,"member_xmt":30,{fields}}}"#
            );
            let record: ParasolidGroupMember = serde_json::from_str(&json).unwrap();
            let expected: serde_json::Value = serde_json::from_str(&json).unwrap();
            GROUP_MEMBER_CLONE_COUNT.with(|count| count.set(0));
            cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
            GROUP_MEMBER_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_current_member_xmt, u32, "current_member_xmt");
