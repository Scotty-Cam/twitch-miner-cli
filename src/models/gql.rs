//! GQL request/response structures.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::constants::GqlOperation;

/// A GQL request body for Twitch's persisted query system.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GqlRequest {
    pub operation_name: String,
    pub extensions: GqlExtensions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GqlExtensions {
    pub persisted_query: PersistedQuery,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedQuery {
    pub version: u8,
    pub sha256_hash: String,
}

impl GqlRequest {
    /// Create a new GQL request from an operation definition.
    pub fn new(operation: &GqlOperation, variables: Option<Value>) -> Self {
        Self {
            operation_name: operation.name.to_string(),
            extensions: GqlExtensions {
                persisted_query: PersistedQuery {
                    version: 1,
                    sha256_hash: operation.sha256.to_string(),
                },
            },
            variables,
        }
    }
}

/// A generic GQL response wrapper.
#[derive(Debug, Clone, Deserialize)]
pub struct GqlResponse<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GqlError {
    pub message: String,
    pub path: Option<Vec<String>>,
}

impl<T> GqlResponse<T> {
    /// Check if the response contains errors.
    pub fn has_errors(&self) -> bool {
        self.errors.as_ref().is_some_and(|e| !e.is_empty())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::gql_operations;

    #[test]
    fn test_gql_request_serialization() {
        let request = GqlRequest::new(&gql_operations::INVENTORY, None);
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["operationName"], "Inventory");
        assert_eq!(json["extensions"]["persistedQuery"]["version"], 1);
        assert_eq!(
            json["extensions"]["persistedQuery"]["sha256Hash"],
            "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b"
        );
        assert!(json.get("variables").is_none());
    }

    #[test]
    fn test_gql_request_with_variables() {
        let variables = serde_json::json!({
            "channelID": "12345",
            "channelLogin": ""
        });
        let request = GqlRequest::new(&gql_operations::CURRENT_DROP, Some(variables));
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["operationName"], "DropCurrentSessionContext");
        assert_eq!(json["variables"]["channelID"], "12345");
    }

    #[test]
    fn test_gql_response_parsing() {
        let json = r#"{
            "data": {"inventory": {"dropCampaignsInProgress": []}},
            "errors": null
        }"#;
        let response: GqlResponse<Value> = serde_json::from_str(json).unwrap();
        assert!(!response.has_errors());
        assert!(response.data.is_some());
    }

    #[test]
    fn test_gql_response_with_errors() {
        let json = r#"{
            "data": null,
            "errors": [{"message": "Not authenticated", "path": ["currentUser"]}]
        }"#;
        let response: GqlResponse<Value> = serde_json::from_str(json).unwrap();
        assert!(response.has_errors());
        assert_eq!(response.errors.unwrap()[0].message, "Not authenticated");
    }
}
