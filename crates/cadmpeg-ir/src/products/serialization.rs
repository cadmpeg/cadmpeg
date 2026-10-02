// SPDX-License-Identifier: Apache-2.0
//! Borrow ordered application-link members during serialization.

use super::{CopyOnChange, LinkState, ProductDefinitionId};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

struct Members<'a>(&'a LinkState);

impl Serialize for Members<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Member<'a> {
            LinkedSubelement { subelement: &'a str },
            ElementComponent { component: &'a ProductDefinitionId },
            ClaimChild { claim: bool },
            CopyOnChange { state: &'a CopyOnChange },
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for subelement in &self.0.linked_subelements { sequence.serialize_element(&Member::LinkedSubelement { subelement })?; }
        if let Some(component) = &self.0.element_component { sequence.serialize_element(&Member::ElementComponent { component })?; }
        if let Some(claim) = self.0.claim_child { sequence.serialize_element(&Member::ClaimChild { claim })?; }
        if let Some(state) = &self.0.copy_on_change { sequence.serialize_element(&Member::CopyOnChange { state })?; }
        sequence.end()
    }
}

impl Serialize for LinkState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> { members: Members<'a> }
        Wire { members: Members(self) }.serialize(serializer)
    }
}
