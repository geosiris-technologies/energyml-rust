//! The eight read-only tools, and the order they are listed in.
//!
//! Each is a unit struct implementing [`Tool`]. They are public so
//! that an application can put one in its own registry next to its tools, or
//! wrap one; they hold no state.

mod common;
mod describe_object;
mod find_resources;
mod get_references;
mod list_arrays;
mod list_dataspaces;
mod list_resources;
mod read_array_window;
mod store_info;

pub use describe_object::DescribeObjectTool;
pub use find_resources::FindResourcesTool;
pub use get_references::GetReferencesTool;
pub use list_arrays::ListArraysTool;
pub use list_dataspaces::ListDataspacesTool;
pub use list_resources::ListResourcesTool;
pub use read_array_window::ReadArrayWindowTool;
pub use store_info::StoreInfoTool;

use crate::tool::Tool;

/// The builtin tools in `tools/list` order: the order of the design, which is
/// the order a model should meet them in (orient, list, find, read, follow,
/// arrays). Part of the contract: a snapshot test guards it.
pub(crate) fn builtin() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(StoreInfoTool),
        Box::new(ListDataspacesTool),
        Box::new(ListResourcesTool),
        Box::new(FindResourcesTool),
        Box::new(DescribeObjectTool),
        Box::new(GetReferencesTool),
        Box::new(ListArraysTool),
        Box::new(ReadArrayWindowTool),
    ]
}
