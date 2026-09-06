// SPDX-License-Identifier: Apache-2.0
//! Intersection declaration forms and derived state-reference sequences.

use serde::{Deserialize, Serialize};
use super::{inline_schema_fields::TermUseValues, xmt_reference::NonNullXmt};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum IntersectionMarker { Type2b, Type2d }
impl TryFrom<u8> for IntersectionMarker {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value { 0x2b => Ok(Self::Type2b), 0x2d => Ok(Self::Type2d), _ => Err("marker: require 0x2b or 0x2d") }
    }
}
impl From<IntersectionMarker> for u8 {
    fn from(value: IntersectionMarker) -> Self { match value { IntersectionMarker::Type2b => 0x2b, IntersectionMarker::Type2d => 0x2d } }
}
#[derive(Clone, Copy)]
pub(crate) enum ReferenceLaneForm { TwoLinks, OneLink }
impl ReferenceLaneForm {
    pub(crate) fn counts(self) -> (usize, usize) { match self { Self::TwoLinks => (2,3), Self::OneLink => (1,4) } }
}
#[derive(Debug, Clone, PartialEq)]
enum Lanes {
    Descending { linked: [NonNullXmt; 2], numeric: Option<TermUseValues> },
    Prior { linked: [NonNullXmt; 2] },
    Anchor { linked: [NonNullXmt; 2] },
    One { linked: NonNullXmt, first: NonNullXmt, start: NonNullXmt },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct Type38State {
    xmt: NonNullXmt,
    node_id: u32,
    leading_references: [u32; 5],
    marker: IntersectionMarker,
    lanes: Lanes,
}
impl Type38State {
    pub(crate) fn new(
        xmt: NonNullXmt, node_id: u32, leading_references: [u32; 5], leading_statuses: [u8; 5],
        marker: IntersectionMarker, linked_references: Vec<NonNullXmt>, state_references: Vec<NonNullXmt>,
        numeric_values: Option<TermUseValues>,
    ) -> Result<Self, &'static str> {
        if leading_statuses[..4] != [1; 4] || !matches!(leading_statuses[4], 0 | 1) {
            return Err("leading_statuses: require four ones followed by zero or one");
        }
        let lanes = match linked_references.as_slice() {
            &[left, right] => {
                let identity = u32::from(xmt);
                identity.checked_add(3).ok_or("xmt: state successors overflow")?;
                let prior = leading_references.into_iter().chain([u32::from(left), u32::from(right)]).fold(0, u32::max);
                prior.checked_add(3).ok_or("leading_references: prior-anchor successors overflow")?;
                let is_sequence = |sequence: [u32; 3]| state_references.iter().copied().map(u32::from).eq(sequence);
                if leading_statuses[4] == 0 {
                    if numeric_values.is_some() { return Err("numeric_values: status-zero anchor cannot carry term-use state"); }
                    let anchor = leading_references[4];
                    if anchor <= 1 { return Err("leading_references: status-zero anchor must be non-null"); }
                    if !is_sequence([anchor + 1, anchor + 2, anchor + 3]) { return Err("state_references: require ascending anchor successors"); }
                    Lanes::Anchor { linked: [left, right] }
                } else if is_sequence([identity + 3, identity + 2, identity + 1]) {
                    Lanes::Descending { linked: [left, right], numeric: numeric_values }
                } else {
                    if numeric_values.is_some() { return Err("numeric_values: require descending identity successors"); }
                    if !is_sequence([prior + 1, prior + 2, prior + 3]) { return Err("state_references: require descending identity or ascending prior successors"); }
                    Lanes::Prior { linked: [left, right] }
                }
            }
            &[linked] => {
                if leading_statuses != [1; 5] { return Err("leading_statuses: one-link form requires all ones"); }
                if numeric_values.is_some() { return Err("numeric_values: one-link form cannot carry term-use state"); }
                let &[first, start, second, third] = state_references.as_slice() else { return Err("state_references: one-link form requires four references"); };
                let anchor = u32::from(start);
                if anchor.checked_add(1) != Some(u32::from(second)) || anchor.checked_add(2) != Some(u32::from(third)) {
                    return Err("state_references: require three ascending terminal references");
                }
                Lanes::One { linked, first, start }
            }
            _ => return Err("linked_references: require one or two references"),
        };
        Ok(Self { xmt, node_id, leading_references, marker, lanes })
    }
    #[cfg(test)]
    pub(crate) fn xmt(&self) -> u32 { self.xmt.into() }
    #[cfg(test)]
    pub(crate) fn marker(&self) -> u8 { self.marker.into() }
    #[cfg(test)]
    pub(crate) fn leading_references(&self) -> [u32; 5] { self.leading_references }
    pub(crate) fn leading_statuses(&self) -> [u8; 5] {
        match self.lanes { Lanes::Anchor { .. } => [1,1,1,1,0], _ => [1; 5] }
    }
    pub(crate) fn linked_references(&self) -> Vec<u32> {
        match self.lanes { Lanes::Descending { linked, .. } | Lanes::Prior { linked } | Lanes::Anchor { linked } => linked.into_iter().map(u32::from).collect(), Lanes::One { linked, .. } => vec![linked.into()] }
    }
    pub(crate) fn state_references(&self) -> Vec<u32> {
        match &self.lanes {
            Lanes::Descending { .. } => { let xmt = u32::from(self.xmt); vec![xmt+3,xmt+2,xmt+1] }
            Lanes::Anchor { .. } => {
                let anchor = self.leading_references[4];
                vec![anchor+1,anchor+2,anchor+3]
            }
            Lanes::Prior { linked } => {
                let anchor = self.leading_references.into_iter().chain(linked.iter().copied().map(u32::from)).fold(0, u32::max);
                vec![anchor+1,anchor+2,anchor+3]
            }
            Lanes::One { first, start, .. } => { let anchor = u32::from(*start); vec![u32::from(*first),anchor,anchor+1,anchor+2] }
        }
    }
    pub(crate) fn numeric_values(&self) -> Option<TermUseValues> {
        match self.lanes { Lanes::Descending { numeric, .. } => numeric, _ => None }
    }
}
fn default_statuses() -> [u8; 5] { [1; 5] }
fn statuses_are_default(statuses: &[u8; 5]) -> bool { *statuses == [1; 5] }
fn deserialize_statuses<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[u8; 5], D::Error> {
    Ok(Option::<[u8; 5]>::deserialize(deserializer)?.unwrap_or_else(default_statuses))
}
#[derive(Serialize, Deserialize)]
struct StateWire {
    xmt: u32,
    node_id: u32,
    leading_references: [u32; 5],
    #[serde(default = "default_statuses", deserialize_with = "deserialize_statuses", skip_serializing_if = "statuses_are_default")]
    leading_statuses: [u8; 5],
    marker: u8,
    linked_references: Vec<u32>,
    state_references: Vec<u32>,
    numeric_values: Option<TermUseValues>,
}
impl TryFrom<StateWire> for Type38State {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(NonNullXmt::try_from(wire.xmt)?, wire.node_id, wire.leading_references, wire.leading_statuses,
            IntersectionMarker::try_from(wire.marker)?,
            wire.linked_references.into_iter().map(NonNullXmt::try_from).collect::<Result<_, _>>().map_err(|_| "linked_references: must be non-null")?,
            wire.state_references.into_iter().map(NonNullXmt::try_from).collect::<Result<_, _>>().map_err(|_| "state_references: must be non-null")?, wire.numeric_values)
    }
}
impl From<Type38State> for StateWire {
    fn from(state: Type38State) -> Self {
        Self { xmt: state.xmt.into(), node_id: state.node_id, leading_references: state.leading_references,
            leading_statuses: state.leading_statuses(), marker: state.marker.into(), linked_references: state.linked_references(),
            state_references: state.state_references(), numeric_values: state.numeric_values() }
    }
}

#[cfg(test)]
mod tests {
    use super::Type38State;
    #[test]
    fn wire_preserves_all_reference_orders_and_rejects_inconsistent_forms() {
        for json in [
            r#"{"xmt":80,"node_id":17,"leading_references":[1,7,8,9,1],"marker":45,"linked_references":[87,12],"state_references":[83,82,81],"numeric_values":null}"#,
            r#"{"xmt":112,"node_id":17,"leading_references":[1,7,8,9,1],"marker":45,"linked_references":[118,119],"state_references":[120,121,122],"numeric_values":null}"#,
            r#"{"xmt":1118,"node_id":3178,"leading_references":[1,3,907,1082,1119],"leading_statuses":[1,1,1,1,0],"marker":45,"linked_references":[1070,1063],"state_references":[1120,1121,1122],"numeric_values":null}"#,
            r#"{"xmt":53,"node_id":2711,"leading_references":[1,778,763,372,1],"marker":43,"linked_references":[381],"state_references":[765,803,804,805],"numeric_values":null}"#,
            r#"{"xmt":40000,"node_id":17,"leading_references":[1,7,8,9,1],"marker":45,"linked_references":[11,12],"state_references":[40003,40002,40001],"numeric_values":[0.5,-0.25,1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0]}"#,
        ] {
            let state: Type38State = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            for (field, value) in [
                ("xmt", serde_json::json!(1)), ("marker", serde_json::json!(4)),
                ("leading_statuses", serde_json::json!([0,1,2,1,1])),
                ("linked_references", serde_json::json!([])),
                ("state_references", serde_json::json!([1,2,3])),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
                wire[field] = value;
                assert!(serde_json::from_value::<Type38State>(wire).unwrap_err().to_string().contains(field));
            }
        }
        let prior = serde_json::json!({"xmt":112,"node_id":17,"leading_references":[1,7,8,9,1],"marker":45,"linked_references":[118,119],"state_references":[120,121,122],"numeric_values":vec![0.0; 11]});
        assert!(serde_json::from_value::<Type38State>(prior).unwrap_err().to_string().contains("numeric_values"));
    }
}
