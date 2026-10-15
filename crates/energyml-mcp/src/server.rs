//! The stdio MCP server: an adapter from [`Registry`] to the `rmcp` SDK.
//!
//! Everything SDK-specific is in this file. [`Tool`], [`Registry`],
//! [`StoreProvider`] and [`Limits`] stay ours (ADR-0021): no `rmcp` type
//! appears in a public signature of this crate, so replacing the SDK, or
//! writing the JSON-RPC loop by hand for stdio, touches this module and
//! nothing else.
//!
//! The adapter does five things and no more:
//!
//! 1. `initialize` / `server/discover`: the [`ServerIdentity`] and the `tools`
//!    capability (the SDK answers both eras of the protocol);
//! 2. `tools/list`: walks [`Registry::tools`] in registration order, with the
//!    annotations for the store the provider gives for an empty
//!    [`RequestInfo`];
//! 3. `tools/call`: an unknown tool is a JSON-RPC protocol error; everything a
//!    tool refuses is a tool result with `isError: true` carrying
//!    `{"refusal": ...}` as structured content and as text; an answer is its
//!    structured content and the same JSON as text;
//! 4. the bridge to blocking stores: each call runs on a blocking thread
//!    (`spawn_blocking` around `futures_executor::block_on`), because the ETP
//!    store blocks inside its `async` methods and an EPC reads files
//!    synchronously;
//! 5. the timeout: [`Limits::call_timeout`] is raced against each call and
//!    answers a `timeout` refusal. A blocking thread cannot be cancelled, so a
//!    timed-out call is *abandoned*, not stopped: it ends on its own and its
//!    answer is dropped.
//!
//! **Stdout is the protocol.** Nothing here prints; a face that wants
//! diagnostics writes them to stderr.
//!
//! ```no_run
//! use std::sync::Arc;
//! use energyml_mcp::server::{ServerIdentity, serve_stdio};
//! use energyml_mcp::{FixedStore, Limits, Registry};
//! # fn store() -> energyml_storage::EpcStore { unimplemented!() }
//!
//! let provider = Arc::new(FixedStore::new(store()));
//! let identity = ServerIdentity::new("energyml-mcp", "0.2.0")
//!     .with_instructions("Read-only access to an EnergyML package.");
//! serve_stdio(Registry::builtin(), provider, Limits::default(), identity)?;
//! # Ok::<(), energyml_mcp::server::ServeError>(())
//! ```

use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool as McpTool, ToolAnnotations,
};
use rmcp::service::{QuitReason, RequestContext};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    CallError, Extensions, Limits, Refusal, RefusalCode, Registry, RequestInfo, StoreInfo, StoreProvider,
    Tool, ToolContext, ToolOutput,
};

/// How long the process waits, once the client is gone, for a store call that
/// is still blocked before it exits anyway. A stuck network read must not keep
/// a closed server alive.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Who the server says it is in `initialize` (name, version, instructions for
/// the model).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServerIdentity {
    /// The `serverInfo.name`.
    pub name: String,
    /// The `serverInfo.version`.
    pub version: String,
    /// Free text a client may show the model once, at the start of a session.
    pub instructions: Option<String>,
}

impl ServerIdentity {
    /// An identity without instructions.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            instructions: None,
        }
    }

    /// With instructions for the model.
    pub fn with_instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(text.into());
        self
    }
}

/// Why serving ended abnormally. A client that closes the connection after a
/// normal session is not an error.
///
/// The causes are kept as text: the SDK's error types are not part of this
/// crate's API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// The async runtime could not be started.
    #[error("could not start the async runtime: {0}")]
    Runtime(#[source] std::io::Error),
    /// The `initialize` / `server/discover` exchange failed, or the client left
    /// before it.
    #[error("the MCP handshake failed: {0}")]
    Handshake(String),
    /// The connection broke or the service task failed after the handshake.
    #[error("the MCP connection failed: {0}")]
    Transport(String),
}

/// A server ready to run: the registry, where stores come from, the bounds,
/// who it says it is, and what an application attached for its own tools.
///
/// [`serve_stdio`] is `Server::new(..).serve_stdio()`. The type exists for the
/// two things the free function does not take: the [`Extensions`] a registered
/// tool reads from [`ToolContext::extensions`], and [`Server::serve_io`], which
/// runs the same server over any pair of byte streams (an in-memory duplex in a
/// test, a socket in an embedding).
pub struct Server {
    registry: Registry,
    provider: Arc<dyn StoreProvider>,
    limits: Limits,
    identity: ServerIdentity,
    extensions: Extensions,
}

impl Server {
    /// A server with no extensions.
    pub fn new(
        registry: Registry,
        provider: Arc<dyn StoreProvider>,
        limits: Limits,
        identity: ServerIdentity,
    ) -> Self {
        Self {
            registry,
            provider,
            limits,
            identity,
            extensions: Extensions::new(),
        }
    }

    /// With what the application's own tools read from
    /// [`ToolContext::extensions`]. The builtin tools never read it.
    pub fn with_extensions(mut self, extensions: Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    /// Serve over stdin and stdout until the client closes. Blocks the calling
    /// thread: it builds its own single-threaded tokio runtime, so a caller that
    /// only wants the registry never sees tokio.
    pub fn serve_stdio(self) -> Result<(), ServeError> {
        // One thread is enough: the work of a call runs on tokio's blocking pool,
        // and the loop only moves JSON. `enable_time` is for the call timeout.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(ServeError::Runtime)?;
        let (stdin, stdout) = rmcp::transport::stdio();
        let outcome = runtime.block_on(self.serve_io(stdin, stdout));
        // Dropping a runtime waits for its blocking threads: a store call stuck
        // on the network would keep the process alive after the client left.
        runtime.shutdown_timeout(SHUTDOWN_GRACE);
        outcome
    }

    /// Serve over this pair of streams, on the runtime the caller is in, until
    /// the peer closes. Frames are JSON-RPC messages separated by newlines, as
    /// on stdio.
    pub async fn serve_io<R, W>(self, reader: R, writer: W) -> Result<(), ServeError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let handler = Handler {
            registry: Arc::new(self.registry),
            provider: self.provider,
            limits: Arc::new(self.limits),
            extensions: Arc::new(self.extensions),
            identity: self.identity,
        };
        let running = handler
            .serve((reader, writer))
            .await
            .map_err(|error| ServeError::Handshake(error.to_string()))?;
        match running.waiting().await {
            Ok(QuitReason::JoinError(error)) => Err(ServeError::Transport(error.to_string())),
            Err(error) => Err(ServeError::Transport(error.to_string())),
            // `Closed` and `Cancelled` are the ends of a normal session; a
            // reason added later is not known to be a failure.
            Ok(_) => Ok(()),
        }
    }
}

/// Serve `registry` over stdin and stdout until the client closes.
///
/// Blocks the calling thread (it builds its own tokio runtime). The store of
/// every request comes from `provider`; for stdio that is a
/// [`FixedStore`](crate::FixedStore).
pub fn serve_stdio(
    registry: Registry,
    provider: Arc<dyn StoreProvider>,
    limits: Limits,
    identity: ServerIdentity,
) -> Result<(), ServeError> {
    Server::new(registry, provider, limits, identity).serve_stdio()
}

/// The `rmcp` handler. Private: the SDK's `ServerHandler` is an implementation
/// detail of this module.
struct Handler {
    // `Arc` because a call moves its share onto a blocking thread, which needs
    // `'static`.
    registry: Arc<Registry>,
    provider: Arc<dyn StoreProvider>,
    limits: Arc<Limits>,
    extensions: Arc<Extensions>,
    identity: ServerIdentity,
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        // Tools only: no resources, prompts, logging or subscriptions in v1, and
        // `listChanged` stays off because the registry is fixed once serving.
        let config =
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
                Implementation::new(self.identity.name.clone(), self.identity.version.clone()),
            );
        match &self.identity.instructions {
            Some(text) => config.with_instructions(text.clone()),
            None => config,
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // The whole list is one page and no cursor is ever issued, so a cursor
        // can only be a client's mistake: say so instead of answering page one.
        if request.is_some_and(|params| params.cursor.is_some()) {
            return Err(McpError::invalid_params(
                "this server lists all its tools in one page and issues no cursor",
                None,
            ));
        }
        // The annotations depend on the store (`openWorldHint` for a remote
        // one). An empty `RequestInfo` is the stdio case; a provider that
        // refuses here refuses the whole list, which MCP allows to vary with
        // the caller's rights.
        let store = self
            .provider
            .store(&RequestInfo::new())
            .await
            .map_err(provider_error)?;
        let info = StoreInfo::of(&*store);
        let tools = self
            .registry
            .tools()
            .map(|tool| describe(tool, &info))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        // Before asking the provider for a store: an unknown name is a protocol
        // error whatever the store would have said. JSON-RPC -32602, the code the
        // MCP specification gives an unknown tool.
        if self.registry.get(&name).is_none() {
            return Err(unknown_tool(&name));
        }
        let args = Value::Object(request.arguments.unwrap_or_default());

        let store = match self.provider.store(&RequestInfo::new()).await {
            Ok(store) => store,
            Err(refusal) => return Ok(error_result(&refusal).into()),
        };

        let default_scope = self.provider.default_scope();
        let registry = Arc::clone(&self.registry);
        let limits = Arc::clone(&self.limits);
        let extensions = Arc::clone(&self.extensions);
        let tool_name = name.clone();
        // The stores' `async` methods may block their thread (an ETP round trip,
        // a file read). On this pool that costs a thread and leaves the loop free
        // to read `notifications/cancelled` and to answer other requests.
        let job = tokio::task::spawn_blocking(move || {
            let ctx =
                ToolContext::new(&*store, &limits, &extensions).with_default_scope(default_scope.as_deref());
            futures_executor::block_on(registry.call(&tool_name, &ctx, args))
        });

        match tokio::time::timeout(self.limits.call_timeout, job).await {
            Err(_elapsed) => Ok(error_result(&timeout_refusal(&name, &self.limits)).into()),
            // A panic in a tool. What it said may carry store text, so it is not
            // repeated; the model is told the call failed, and the operator has
            // the panic message on stderr from the default hook.
            Ok(Err(_join_error)) => Ok(error_result(
                &Refusal::new(RefusalCode::Other, "the tool failed unexpectedly")
                    .with_operation(name)
                    .with_hint("retry once; if it fails again, report it to whoever runs the server"),
            )
            .into()),
            Ok(Ok(Ok(output))) => Ok(success_result(output).into()),
            Ok(Ok(Err(CallError::Refused(refusal)))) => Ok(error_result(&refusal).into()),
            // Exhaustive: `CallError` is this crate's, so a variant added to it
            // stops this from compiling until it is given its answer here.
            Ok(Ok(Err(CallError::UnknownTool(unknown)))) => Err(unknown_tool(&unknown)),
        }
    }
}

/// A tool as MCP lists it.
fn describe(tool: &dyn Tool, info: &StoreInfo) -> Result<McpTool, McpError> {
    // A schema that is not an object is a defect of the tool (an application's,
    // since the builtin ones are guarded by the snapshot test). Listing it with
    // a made-up schema would draw something the tool never promised: refuse.
    let Value::Object(input) = tool.input_schema() else {
        return Err(McpError::internal_error(
            format!("the input schema of tool `{}` is not a JSON object", tool.name()),
            None,
        ));
    };
    let hints = tool.annotations(info);
    let mut described = McpTool::new(tool.name(), tool.description(), Arc::new(input))
        .with_title(tool.title())
        .with_annotations(
            ToolAnnotations::with_title(hints.title.clone())
                .read_only(hints.read_only_hint)
                .open_world(hints.open_world_hint),
        );
    match tool.output_schema() {
        None => {}
        Some(Value::Object(output)) => described = described.with_raw_output_schema(Arc::new(output)),
        Some(_) => {
            return Err(McpError::internal_error(
                format!("the output schema of tool `{}` is not a JSON object", tool.name()),
                None,
            ));
        }
    }
    Ok(described)
}

/// A successful answer: the structured value, and the same JSON as text for the
/// clients that read only that (the specification asks for both).
fn success_result(output: ToolOutput) -> CallToolResult {
    let mut result = CallToolResult::structured(output.structured);
    // `structured` serialises the value itself; the text of the output is what
    // the tool (and `ToolOutput::new`) promised agrees with it, so use that one.
    result.content = vec![ContentBlock::text(output.text)];
    result
}

/// A refusal as a tool execution error: `isError: true`, `{"refusal": ...}` as
/// structured content and as text.
fn error_result(refusal: &Refusal) -> CallToolResult {
    let output = ToolOutput::from_refusal(refusal);
    let mut result = CallToolResult::structured_error(output.structured);
    result.content = vec![ContentBlock::text(output.text)];
    result
}

fn timeout_refusal(name: &str, limits: &Limits) -> Refusal {
    Refusal::new(RefusalCode::Timeout, "the call took longer than the server allows")
        .with_operation(name)
        .with_hint(format!(
            "the limit is {} s: restrict the request (a narrower scope, a type filter, a smaller window) and retry",
            limits.call_timeout.as_secs()
        ))
}

fn unknown_tool(name: &str) -> McpError {
    McpError::invalid_params(format!("unknown tool `{name}`"), None)
}

/// A provider that refuses to give a store, outside of a tool call (listing).
fn provider_error(refusal: Refusal) -> McpError {
    McpError::internal_error(refusal.to_string(), Some(refusal.to_value()))
}
