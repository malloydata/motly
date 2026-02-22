use std::collections::BTreeMap;

/// A scalar value in the MOTLY tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    String(String),
    Number(f64),
    Boolean(bool),
    /// ISO 8601 date string (e.g. "2024-01-15" or "2024-01-15T10:30:00Z")
    Date(String),
}

/// The value of a node's `eq` field: scalar, array, reference, or env ref.
#[derive(Debug, Clone, PartialEq)]
pub enum EqValue {
    Scalar(Scalar),
    Array(Vec<MOTLYNode>),
    /// A reference to another node: `{ "linkTo": "$path" }`
    Reference(String),
    /// An environment variable reference: `{ "env": "NAME" }`
    EnvRef(String),
}

/// A node in the MOTLY output tree. References now live in the eq slot
/// as EqValue::Reference, so MOTLYNode is just MOTLYValue.
pub type MOTLYNode = MOTLYValue;

/// A value node in the MOTLY output tree (has eq, properties, deleted).
#[derive(Debug, Clone, PartialEq)]
pub struct MOTLYValue {
    pub eq: Option<EqValue>,
    pub properties: Option<BTreeMap<String, MOTLYNode>>,
    pub deleted: bool,
}

impl MOTLYValue {
    pub fn new() -> Self {
        MOTLYValue {
            eq: None,
            properties: None,
            deleted: false,
        }
    }

    pub fn with_eq(eq: EqValue) -> Self {
        MOTLYValue {
            eq: Some(eq),
            properties: None,
            deleted: false,
        }
    }

    pub fn deleted() -> Self {
        MOTLYValue {
            eq: None,
            properties: None,
            deleted: true,
        }
    }

    /// Get or create the properties map.
    pub fn get_or_create_properties(&mut self) -> &mut BTreeMap<String, MOTLYNode> {
        self.properties.get_or_insert_with(BTreeMap::new)
    }

    /// Serialize to compact JSON.
    pub fn to_json(&self) -> String {
        crate::json::to_json(self)
    }

    /// Serialize to pretty-printed JSON (2-space indent).
    pub fn to_json_pretty(&self) -> String {
        crate::json::to_json_pretty(self)
    }

    /// Check if this node's eq is a reference (linkTo).
    pub fn is_ref(&self) -> bool {
        matches!(&self.eq, Some(EqValue::Reference(_)))
    }

    /// Check if this node's eq is an env reference.
    pub fn is_env_ref(&self) -> bool {
        matches!(&self.eq, Some(EqValue::EnvRef(_)))
    }
}

impl Default for MOTLYValue {
    fn default() -> Self {
        Self::new()
    }
}
