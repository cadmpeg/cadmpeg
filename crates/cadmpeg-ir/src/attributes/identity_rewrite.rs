// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{AttributeTarget, AttributeValue, SourceAttribute};

rewrite_enum!(AttributeTarget, []; {
    Document,
    Body(field0),
    Face(field0),
    Shell(field0),
    Loop(field0),
    Coedge(field0),
    Edge(field0),
    Vertex(field0),
});
rewrite_enum!(AttributeValue, []; {
    Integer(field0),
    Float(field0),
    String(field0),
    Boolean(field0),
    Reference(field0),
    Vector(field0),
});
rewrite_record!(SourceAttribute, []; {id, target, name, values});
