//! Where the store of a call comes from: the one point that varies between the
//! ways of running this server.
//!
//! A server started on a file or a remote session serves one store, opened at
//! launch ([`FixedStore`]). A server behind HTTP serves a store per request,
//! restricted to what the caller's account may see; it implements
//! [`StoreProvider`] itself. The tools do not know the difference.

use std::sync::Arc;

use async_trait::async_trait;
use energyml_storage::EnergymlStorage;

use crate::refusal::Refusal;

/// What the transport knows about one request, for a provider that cares.
///
/// Empty for stdio. `#[non_exhaustive]`: an HTTP transport will add the headers
/// and the account, and that is not a breaking change.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RequestInfo {}

impl RequestInfo {
    /// The request of a transport that knows nothing about it (stdio).
    pub fn new() -> Self {
        Self::default()
    }
}

/// Gives the store a request reads.
// `async_trait` marks the boxed future `#[must_use]`, which clippy reads as
// ours (`double_must_use`): the same allowance as `EnergymlStorage`.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait StoreProvider: Send + Sync {
    /// The store for this request, or a refusal (no rights, no session).
    async fn store(&self, request: &RequestInfo) -> Result<Arc<dyn EnergymlStorage + Send + Sync>, Refusal>;
}

/// One store, opened once: the stdio case.
#[derive(Clone)]
pub struct FixedStore(Arc<dyn EnergymlStorage + Send + Sync>);

impl FixedStore {
    /// Serve this store.
    pub fn new(store: impl EnergymlStorage + Send + Sync + 'static) -> Self {
        Self(Arc::new(store))
    }

    /// Serve a store that is already shared.
    pub fn from_arc(store: Arc<dyn EnergymlStorage + Send + Sync>) -> Self {
        Self(store)
    }
}

impl std::fmt::Debug for FixedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FixedStore").field(&self.0.store_kind()).finish()
    }
}

#[async_trait]
impl StoreProvider for FixedStore {
    async fn store(&self, _request: &RequestInfo) -> Result<Arc<dyn EnergymlStorage + Send + Sync>, Refusal> {
        Ok(Arc::clone(&self.0))
    }
}
