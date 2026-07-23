use std::collections::HashMap;
use std::sync::Arc;

use super::BackendFactory;

#[derive(Default)]
pub struct BackendRegistry {
    by_scheme: HashMap<String, Vec<Arc<dyn BackendFactory>>>,
    by_name: HashMap<String, Arc<dyn BackendFactory>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Arc<dyn BackendFactory>) {
        let scheme = factory.scheme().to_lowercase();
        let name = factory.name().to_lowercase();
        self.by_scheme
            .entry(scheme)
            .or_default()
            .push(Arc::clone(&factory));
        self.by_name.insert(name, factory);
    }

    pub fn get_by_scheme(&self, scheme: &str) -> Option<&Arc<dyn BackendFactory>> {
        self.by_scheme
            .get(&scheme.to_lowercase())
            .and_then(|factories| factories.first())
    }

    pub fn get_by_name(&self, name: &str) -> Option<&Arc<dyn BackendFactory>> {
        self.by_name.get(&name.to_lowercase())
    }

    pub fn resolve(
        &self,
        driver: Option<&str>,
        url: Option<&str>,
        default: &str,
    ) -> Option<&Arc<dyn BackendFactory>> {
        if let Some(d) = driver {
            if let Some(f) = self.get_by_name(d) {
                return Some(f);
            }
            if let Some(f) = self.get_by_scheme(d) {
                return Some(f);
            }
        }
        if let Some(u) = url {
            if let Some(scheme_end) = u.find("://") {
                let scheme = &u[..scheme_end];
                if let Some(f) = self.get_by_scheme(scheme) {
                    return Some(f);
                }
            }
        }
        self.get_by_name(default)
            .or_else(|| self.get_by_scheme(default))
    }
}
