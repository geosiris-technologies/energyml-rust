//! **What an operation declares, as one JSON document** — the wire form of
//! `AnyOperation`, assembled here once so that the front end, the command line and
//! any script read the same keys.
//!
//! It used to be assembled inside a WebAssembly front end for the creation form
//! alone (a workflow server needs the same description without a front end).
//! A face that built its own would be a second place to forget a key, so both
//! call [`declaration`].
//!
//! The keys: `name`, `describe`, `makes`, `recipe`, `category`, `also`
//! ([`AnyOperation::json`]), then `needs` (what it asks, on a bag), `takes`
//! (the inputs it declares), `reports` (what its activity records that is
//! neither a choice nor an output) and `gives` (what it writes, by role —
//! [`Output`](crate::operation::Output)). Every slot is `Slot::json()`.
//!
//! A pure declaration is the one on the **empty bag**: `needs` must answer
//! there, and suggestions are simply absent.

use serde_json::{Value, json};

use crate::operation::{AnyOperation, Registry, Slot};
use crate::pieces::Pieces;

/// One operation, with `needs` already computed for the bag the caller holds.
/// What the front end calls, since it judged the bag itself.
pub fn declaration_with(operation: &dyn AnyOperation, needs: &[Slot]) -> Value {
    let slots = |list: &[Slot]| Value::Array(list.iter().map(Slot::json).collect());
    let mut out = operation.json();
    out["needs"] = slots(needs);
    // What the operation takes, beside what it asks: two lists, and
    // rendered even for an operation unfit on this bag, so a form can ask for
    // the inputs before there is any bag.
    out["takes"] = slots(&operation.takes());
    out["reports"] = slots(&operation.reports());
    out["gives"] = json!(operation.gives());
    out
}

/// One operation on a given bag.
pub fn declaration(operation: &dyn AnyOperation, pieces: &Pieces) -> Value {
    declaration_with(operation, &operation.needs(pieces))
}

/// One operation by name, on the empty bag — `None` when no operation has it.
pub fn declaration_of(registry: &Registry, name: &str) -> Option<Value> {
    registry
        .get(name)
        .map(|operation| declaration(operation, &Pieces::default()))
}

/// Every operation, on the empty bag, in the registry's order.
pub fn declarations(registry: &Registry) -> Value {
    let empty = Pieces::default();
    Value::Array(
        registry
            .iter()
            .map(|operation| declaration(operation, &empty))
            .collect(),
    )
}
