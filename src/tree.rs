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

/// The value of a node's `eq` field: either a scalar or an array of MOTLYNode.
#[derive(Debug, Clone, PartialEq)]
pub enum EqValue {
    Scalar(Scalar),
    Array(Vec<MOTLYNode>),
}

/// A node in the MOTLY output tree: either a value or a reference.
#[derive(Debug, Clone, PartialEq)]
pub enum MOTLYNode {
    Value(MOTLYValue),
    Ref(MOTLYRef),
}

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
}

impl Default for MOTLYValue {
    fn default() -> Self {
        Self::new()
    }
}

/// A link (reference) in the MOTLY output tree.
#[derive(Debug, Clone, PartialEq)]
pub struct MOTLYRef {
    pub link_to: String,
}
