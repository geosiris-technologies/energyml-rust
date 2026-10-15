//! The registry of tools: the caller's first, then the builtin ones.
//!
//! A registry rather than a `match`, because the list will grow: an application
//! adds the tools that drive its own views, a server adds the tools its rights
//! allow. The rule is the repository's: what the caller registers is tried
//! **first**.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value;

use crate::refusal::{Refusal, RefusalCode};
use crate::tool::{Tool, ToolContext, ToolOutput};
use crate::tools;

/// Why a call produced no answer.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CallError {
    /// No tool of this name. A protocol error (JSON-RPC), not a tool error.
    UnknownTool(String),
    /// The tool refused. A tool execution error (`isError: true`) the model can
    /// read.
    Refused(Refusal),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::UnknownTool(name) => write!(f, "unknown tool `{name}`"),
            CallError::Refused(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for CallError {}

impl From<Refusal> for CallError {
    fn from(refusal: Refusal) -> Self {
        CallError::Refused(refusal)
    }
}

/// The tools of a server, in the order `tools/list` shows them.
///
/// Registration order, deterministic: the caller's tools in the order they were
/// added, then the builtin ones in their fixed order.
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
    /// How many of `tools` the caller added; they sit at the front.
    caller: usize,
}

impl Registry {
    /// No tool at all, for a server that offers only its own.
    pub fn empty() -> Self {
        Self {
            tools: Vec::new(),
            caller: 0,
        }
    }

    /// The eight read-only tools of the design, in this order:
    /// `energyml_store_info`, `energyml_list_dataspaces`,
    /// `energyml_list_resources`, `energyml_find_resources`,
    /// `energyml_describe_object`, `energyml_get_references`,
    /// `energyml_list_arrays`, `energyml_read_array_window`.
    pub fn builtin() -> Self {
        Self {
            tools: tools::builtin(),
            caller: 0,
        }
    }

    /// Add a tool of the caller, tried before the builtin ones.
    ///
    /// Listing order: caller tools in the order added, then builtin. **A name
    /// already present is shadowed**: the first tool registered under a name is
    /// the one [`Registry::get`] and [`Registry::call`] find, and the later ones
    /// are neither listed nor callable, because MCP tool names are unique. A
    /// caller tool that reuses `energyml_describe_object` therefore replaces
    /// the builtin one, and two caller tools of one name keep the earlier.
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.insert(self.caller, tool);
        self.caller += 1;
        self
    }

    /// The tools to list, in order, each name once (a shadowed tool is skipped).
    pub fn tools(&self) -> impl Iterator<Item = &dyn Tool> {
        let mut seen = HashSet::new();
        let listed: Vec<&dyn Tool> = self
            .tools
            .iter()
            .map(|tool| &**tool)
            .filter(|tool| seen.insert(tool.name()))
            .collect();
        listed.into_iter()
    }

    /// The tool of this name: the first registered.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .map(|tool| &**tool)
            .find(|tool| tool.name() == name)
    }

    /// Call a tool.
    ///
    /// Two checks belong here and not in each tool: an unknown name is a
    /// protocol error, and an answer larger than
    /// [`Limits::max_response_bytes`](crate::Limits::max_response_bytes) is
    /// refused `too_large` rather than sent whole or cut mid-structure.
    ///
    /// **No timeout is applied**: there is no runtime in this crate. The adapter
    /// that owns one races this future against
    /// [`Limits::call_timeout`](crate::Limits::call_timeout) and answers with a
    /// [`RefusalCode::Timeout`] refusal.
    pub async fn call(
        &self,
        name: &str,
        ctx: &ToolContext<'_>,
        args: Value,
    ) -> Result<ToolOutput, CallError> {
        let tool = self
            .get(name)
            .ok_or_else(|| CallError::UnknownTool(name.to_string()))?;
        let output = tool.call(ctx, args).await.map_err(CallError::Refused)?;
        if output.text.len() > ctx.limits.max_response_bytes {
            return Err(CallError::Refused(
                Refusal::new(
                    RefusalCode::TooLarge,
                    "the answer is larger than the server allows",
                )
                .with_operation(name)
                .with_hint("narrow the request: a smaller `limit`, a filter, or a smaller window"),
            ));
        }
        Ok(output)
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.tools().map(|tool| tool.name()))
            .finish()
    }
}
