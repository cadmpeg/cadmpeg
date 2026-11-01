// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Dimension, SolverSettings, SolverTarget, SolverValue};

rewrite_enum!(SolverTarget, []; {
    Scene {scene},
    Occurrence {occurrence},
    Definition {definition},
    Body {body},
    Collider {collider},
    Joint {joint},
    PhysicalMaterial {material},
    Articulation {articulation},
});
rewrite_scalar!(Dimension);
rewrite_enum!(SolverValue, []; {
    Bool {value},
    Integer {value},
    Real {value, dimension},
    Unbounded {negative},
    Reals {values, dimension},
    Token {value},
    Tokens {values},
    Targets {targets},
});
rewrite_record!(SolverSettings, []; {id, solver, schema, target, values, native_ref});
