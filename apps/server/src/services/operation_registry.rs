use crate::db::PostgresResourceStore;
use crate::error::{Error, Result};
use crate::models::{OperationContext, OperationMetadata, OperationParameter, Parameters};
use serde_json::Value as JsonValue;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OperationRegistry {
    store: Arc<PostgresResourceStore>,
    cache: Arc<RwLock<HashMap<String, Vec<OperationMetadata>>>>,
}

impl OperationRegistry {
    pub fn new(store: Arc<PostgresResourceStore>) -> Self {
        Self {
            store,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn load_definitions(&self) -> Result<()> {
        // Load OperationDefinitions directly from the resources table.
        // (ResourceStore::search() is intentionally optimized for indexed FHIR search and may be
        // disabled or stubbed in some configurations.)
        let rows = sqlx::query(
            "SELECT resource
             FROM resources
             WHERE resource_type = 'OperationDefinition'
               AND is_current = TRUE
               AND deleted = FALSE",
        )
        .fetch_all(&self.store.pool)
        .await
        .map_err(Error::Database)?;

        let mut cache = self.cache.write().await;
        cache.clear();

        for row in rows {
            let resource: JsonValue = row.get("resource");
            if let Some(metadata) = self.parse_operation_definition(&resource)? {
                cache
                    .entry(metadata.code.clone())
                    .or_insert_with(Vec::new)
                    .push(metadata);
            }
        }

        let def_count: usize = cache.values().map(|v| v.len()).sum();
        tracing::info!(
            "Loaded {} operation definitions ({} codes)",
            def_count,
            cache.len()
        );
        Ok(())
    }

    pub async fn list_all(&self) -> Vec<OperationMetadata> {
        let cache = self.cache.read().await;
        cache.values().flatten().cloned().collect()
    }

    pub async fn find_operation(
        &self,
        code: &str,
        context: &OperationContext,
    ) -> Result<Option<OperationMetadata>> {
        let cache = self.cache.read().await;

        if let Some(candidates) = cache.get(code) {
            for metadata in candidates {
                if self.context_matches(metadata, context) {
                    return Ok(Some(metadata.clone()));
                }
            }
        }

        Ok(None)
    }

    pub async fn validate_parameters(
        &self,
        metadata: &OperationMetadata,
        parameters: &Parameters,
    ) -> Result<()> {
        // Minimal validation:
        // - Required parameters (min > 0) for IN/BOTH
        // - Max cardinality
        for def in metadata.parameters.iter().filter(|p| {
            matches!(
                p.use_type,
                crate::models::ParameterUse::In | crate::models::ParameterUse::Both
            )
        }) {
            let count = parameters.count_parameters(&def.name);
            if count < def.min {
                return Err(Error::Validation(format!(
                    "Missing required parameter: {}",
                    def.name
                )));
            }

            if def.max != "*" {
                let Ok(max) = def.max.parse::<usize>() else {
                    continue;
                };
                if count > max {
                    return Err(Error::Validation(format!(
                        "Parameter '{}' exceeds max cardinality {}",
                        def.name, def.max
                    )));
                }
            }
        }
        Ok(())
    }

    fn parse_operation_definition(
        &self,
        resource: &JsonValue,
    ) -> Result<Option<OperationMetadata>> {
        let code = resource
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidResource("Missing code".to_string()))?;

        let name = resource
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(code);

        let system = resource
            .get("system")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let type_level = resource
            .get("type")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let instance = resource
            .get("instance")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let affects_state = resource
            .get("affectsState")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let type_contexts: Vec<String> = resource
            .get("resource")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let parameters: Vec<OperationParameter> = resource
            .get("parameter")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Some(OperationMetadata {
            name: name.to_string(),
            code: code.to_string(),
            system,
            type_level,
            type_contexts,
            instance,
            parameters,
            affects_state,
        }))
    }

    fn context_matches(&self, metadata: &OperationMetadata, context: &OperationContext) -> bool {
        match context {
            OperationContext::System => metadata.system,
            OperationContext::Type(resource_type) => {
                metadata.type_level && metadata.type_contexts.contains(resource_type)
            }
            OperationContext::Instance(resource_type, _) => {
                metadata.instance && metadata.type_contexts.contains(resource_type)
            }
        }
    }
}
