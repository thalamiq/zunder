use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Operation context (where the operation can be invoked)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationContext {
    System,
    Type(String),
    Instance(String, String),
}

/// Parameter usage direction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterUse {
    In,
    Out,
    #[serde(rename = "both")]
    Both,
}

/// Operation parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationParameter {
    pub name: String,
    #[serde(rename = "use")]
    pub use_type: ParameterUse,
    pub min: usize,
    pub max: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "searchType")]
    pub search_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<Vec<OperationParameter>>,
}

/// Parsed operation definition metadata
#[derive(Debug, Clone, Serialize)]
pub struct OperationMetadata {
    pub name: String,
    pub code: String,
    pub system: bool,
    pub type_level: bool,
    pub type_contexts: Vec<String>,
    pub instance: bool,
    pub parameters: Vec<OperationParameter>,
    pub affects_state: bool,
}

/// Operation invocation request
#[derive(Debug)]
pub struct OperationRequest {
    pub operation_name: String,
    pub context: OperationContext,
    pub parameters: Parameters,
}

/// FHIR Parameters resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<Vec<Parameter>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(flatten)]
    pub value: ParameterValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    Resource {
        resource: JsonValue,
    },
    Parts {
        part: Vec<Parameter>,
    },
    /// FHIR JSON "value[x]" parameter content, e.g. `{ "valueString": "..." }`
    Value(HashMap<String, JsonValue>),
}

impl Parameters {
    pub fn new() -> Self {
        Self {
            resource_type: "Parameters".to_string(),
            parameter: None,
        }
    }

    pub fn add_value_string(&mut self, name: String, value: String) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueString".to_string(),
                JsonValue::String(value),
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_boolean(&mut self, name: String, value: bool) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueBoolean".to_string(),
                JsonValue::Bool(value),
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_integer(&mut self, name: String, value: i64) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueInteger".to_string(),
                JsonValue::Number(value.into()),
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_code(&mut self, name: String, value: String) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueCode".to_string(),
                JsonValue::String(value),
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_uri(&mut self, name: String, value: String) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueUri".to_string(),
                JsonValue::String(value),
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_coding(&mut self, name: String, coding: JsonValue) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([("valueCoding".to_string(), coding)])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_value_codeable_concept(&mut self, name: String, concept: JsonValue) {
        let param = Parameter {
            name,
            value: ParameterValue::Value(HashMap::from([(
                "valueCodeableConcept".to_string(),
                concept,
            )])),
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_resource(&mut self, name: String, resource: JsonValue) {
        let param = Parameter {
            name,
            value: ParameterValue::Resource { resource },
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn add_parts(&mut self, name: String, part: Vec<Parameter>) {
        let param = Parameter {
            name,
            value: ParameterValue::Parts { part },
        };
        self.parameter.get_or_insert_with(Vec::new).push(param);
    }

    pub fn get_parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameter.as_ref()?.iter().find(|p| p.name == name)
    }

    pub fn get_value(&self, name: &str) -> Option<&JsonValue> {
        match &self.get_parameter(name)?.value {
            ParameterValue::Value(map) if map.len() == 1 => map.values().next(),
            _ => None,
        }
    }

    pub fn get_values(&self, name: &str) -> Vec<&JsonValue> {
        let Some(params) = self.parameter.as_ref() else {
            return Vec::new();
        };
        params
            .iter()
            .filter(|p| p.name == name)
            .filter_map(|p| match &p.value {
                ParameterValue::Value(map) if map.len() == 1 => map.values().next(),
                _ => None,
            })
            .collect()
    }

    pub fn get_resource(&self, name: &str) -> Option<&JsonValue> {
        match &self.get_parameter(name)?.value {
            ParameterValue::Resource { resource } => Some(resource),
            _ => None,
        }
    }

    pub fn count_parameters(&self, name: &str) -> usize {
        self.parameter
            .as_ref()
            .map(|ps| ps.iter().filter(|p| p.name == name).count())
            .unwrap_or(0)
    }

    pub fn all_parameters(&self) -> impl Iterator<Item = &Parameter> {
        self.parameter.as_deref().unwrap_or(&[]).iter()
    }
}

impl Default for Parameters {
    fn default() -> Self {
        Self::new()
    }
}

/// Operation execution result
#[derive(Debug)]
pub enum OperationResult {
    Resource(JsonValue),
    Parameters(Parameters),
    OperationOutcome(JsonValue),
    NoContent,
}
