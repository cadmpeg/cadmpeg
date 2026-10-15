// SPDX-License-Identifier: Apache-2.0
//! PMARK nodes and nonempty packet contents.

use super::pmark_state::PmarkState;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<PmarkState>")]
pub(crate) struct PmarkNodes(Vec<PmarkState>);

impl Serialize for PmarkNodes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl PmarkNodes {
    pub(super) fn new(ctx: &DecodeContext<'_>, first: PmarkState) -> Result<Self, CodecError> {
        Ok(Self(ctx.alloc_filled(1, first, "NX PMARK nodes")?))
    }
    pub(super) fn push(
        &mut self,
        ctx: &DecodeContext<'_>,
        node: PmarkState,
    ) -> Result<(), CodecError> {
        ctx.reserve_vec(&mut self.0, 1, "NX PMARK nodes")?;
        self.0.push(node);
        Ok(())
    }
    pub(crate) fn as_slice(&self) -> &[PmarkState] {
        &self.0
    }
}

impl TryFrom<Vec<PmarkState>> for PmarkNodes {
    type Error = &'static str;
    fn try_from(nodes: Vec<PmarkState>) -> Result<Self, Self::Error> {
        if nodes.is_empty() {
            return Err("nodes: require at least one node");
        }
        Ok(Self(nodes))
    }
}

#[cfg(test)]
std::thread_local! {
    static PMARK_NODES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<PmarkNodes> for Vec<PmarkState> {
    fn from(nodes: PmarkNodes) -> Self {
        PMARK_NODES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        nodes.0
    }
}

#[cfg(test)]
mod tests {
    use super::{PmarkNodes, PMARK_NODES_INTO_WIRE_COUNT};

    #[test]
    fn pmark_nodes_preserve_array_wire_and_reject_empty_packets() {
        let json = r#"[{"xmt":2,"preceding":3,"first_following":4,"next_sibling":1,"prev_sibling":1,"n_new_nodes":34,"n_del_nodes":6,"n_copy_mod_nodes":11,"delta_key":22362,"delta_is_forward":false,"identifier":321}]"#;
        let nodes: PmarkNodes = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&nodes).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&nodes).unwrap(),
            serde_json::to_vec(&Vec::<super::PmarkState>::from(nodes.clone())).unwrap()
        );
        assert!(serde_json::from_str::<PmarkNodes>("[]")
            .unwrap_err()
            .to_string()
            .contains("nodes"));
    }

    #[test]
    fn pmark_nodes_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            nodes: &'a PmarkNodes,
        }
        let nodes: PmarkNodes = serde_json::from_str(
            r#"[{"xmt":2,"preceding":3,"first_following":4,"next_sibling":1,"prev_sibling":1,"n_new_nodes":34,"n_del_nodes":6,"n_copy_mod_nodes":11,"delta_key":22362,"delta_is_forward":false,"identifier":321}]"#,
        )
        .unwrap();
        let record = Record {
            id: "nx:deltas:pmark-nodes#0",
            nodes: &nodes,
        };
        PMARK_NODES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": "nx:deltas:pmark-nodes#0", "nodes": nodes}),
        );
        PMARK_NODES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
