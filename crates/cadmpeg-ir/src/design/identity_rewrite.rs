// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by the design references that model-class
//! records hold.

use super::names::NativeName;
use super::refs::{ConfigurationRef, EntityRef, Evaluated, Member, Position, StateRef};
use super::selection::{OccurrencePath, PathStep, StepRef};

rewrite_enum!(EntityRef, []; {
    Body(field0),
    Face(field0),
    Edge(field0),
    Vertex(field0),
    Surface(field0),
    Curve(field0),
    Subd(field0),
});
rewrite_enum!(ConfigurationRef, []; {
    Local(field0),
    Named(field0),
});
rewrite_enum!(Member, []; {
    Feature(field0),
    Container(field0),
});
rewrite_enum!(Position, []; {
    Start(field0),
    After(field0),
});
rewrite_enum!(Evaluated, []; {
    State(field0),
    Tool(field0),
});
rewrite_record!(StateRef, []; {configuration, of});
rewrite_record!(OccurrencePath, []; (field0));
rewrite_record!(PathStep, []; {occurrence, configuration});
rewrite_enum!(StepRef, []; {
    Occurrence(field0),
    Native(field0),
});
rewrite_scalar!(NativeName);
