//! Pure-data solver primitives with no byte knowledge.

pub(crate) mod incidence;
pub(crate) mod matching;
pub(crate) mod mesh_quotient;
pub(crate) mod missing_edge;
pub(crate) mod union_find;

pub(crate) use union_find::UnionFind;
