// ─── Backend Registry ───────────────────────────────────────────────
//
// Maps URL schemes and driver names to BackendFactory implementations.
// Used by the CLI and MCP server to route connections to the correct backend.

use std::collections::HashMap;
use std::sync::Arc;

use super::BackendFactory;

/// Registry of all available database backends.
/// Factories are registered by scheme (e.g. "mysql", "oracle") and name.
#[derive(Default)]
pub struct BackendRegistry {
    /// scheme → factory (for URL-based routing: mysql://, oracle://)
    by_scheme: HashMap<String, Arc<dyn BackendFactory>>,
    /// driver name → factory (for config-based routing: driver = "oracle")
    by_name: HashMap<String, Arc<dyn BackendFactory>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend factory. It will be accessible by both
    /// URL scheme and driver name.
    pub fn register(&mut self, factory: Arc<dyn BackendFactory>) {
        let scheme = factory.scheme().to_lowercase();
        let name = factory.name().to_lowercase();
        self.by_scheme.insert(scheme, Arc::clone(&factory));
        self.by_name.insert(name, factory);
    }

    /// Look up a factory by URL scheme (e.g. "mysql", "oracle").
    pub fn get_by_scheme(&self, scheme: &str) -> Option<&Arc<dyn BackendFactory>> {
        self.by_scheme.get(&scheme.to_lowercase())
    }

    /// Look up a factory by driver name (e.g. "MySQL", "mysql", "Oracle").
    pub fn get_by_name(&self, name: &str) -> Option<&Arc<dyn BackendFactory>> {
        self.by_name.get(&name.to_lowercase())
    }

    /// Try to resolve a backend: first by explicit driver name,
    /// then by URL scheme prefix, then fall back to default.
    pub fn resolve(
        &self,
        driver: Option<&str>,
        url: Option<&str>,
        default: &str,
    ) -> Option<&Arc<dyn BackendFactory>> {
        // 1. Explicit driver field in config
        if let Some(d) = driver {
            if let Some(f) = self.get_by_name(d) {
                return Some(f);
            }
            // Also try as scheme
            if let Some(f) = self.get_by_scheme(d) {
                return Some(f);
            }
        }

        // 2. URL scheme detection
        if let Some(u) = url {
            if let Some(scheme_end) = u.find("://") {
                let scheme = &u[..scheme_end];
                if let Some(f) = self.get_by_scheme(scheme) {
                    return Some(f);
                }
            }
        }

        // 3. Default
        self.get_by_name(default)
            .or_else(|| self.get_by_scheme(default))
    }
}
