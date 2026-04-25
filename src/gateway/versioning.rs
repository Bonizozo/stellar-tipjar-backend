use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ApiVersion {
    pub version: String,
    pub deprecated: bool,
    pub sunset_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiVersionManager {
    versions: HashMap<String, ApiVersion>,
    current_version: String,
}

impl ApiVersionManager {
    pub fn new(current_version: String) -> Self {
        Self {
            versions: HashMap::new(),
            current_version,
        }
    }

    pub fn register_version(&mut self, version: String, deprecated: bool, sunset_date: Option<String>) {
        self.versions.insert(
            version.clone(),
            ApiVersion {
                version,
                deprecated,
                sunset_date,
            },
        );
    }

    pub fn get_version(&self, version: &str) -> Option<&ApiVersion> {
        self.versions.get(version)
    }

    pub fn is_version_supported(&self, version: &str) -> bool {
        self.versions
            .get(version)
            .map(|v| !v.deprecated)
            .unwrap_or(false)
    }

    pub fn get_current_version(&self) -> &str {
        &self.current_version
    }

    pub fn get_deprecated_versions(&self) -> Vec<&ApiVersion> {
        self.versions
            .values()
            .filter(|v| v.deprecated)
            .collect()
    }
}
