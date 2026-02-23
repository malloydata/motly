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

/// The value of a node's `eq` field: scalar, array, or env ref.
/// References are NOT in eq — they are a separate `MOTLYPropertyValue` variant.
#[derive(Debug, Clone, PartialEq)]
pub enum EqValue {
    Scalar(Scalar),
    Array(Vec<MOTLYPropertyValue>),
    /// An environment variable reference: `{ "env": "NAME" }`
    EnvRef(String),
}

/// What a property or array element leads to: either a node or a link reference.
///
/// A `Ref` means "this IS that other node" — no own value, no own properties.
/// A `Node` is a full node with optional eq, properties, and deleted flag.
#[derive(Debug, Clone, PartialEq)]
pub enum MOTLYPropertyValue {
    Node(MOTLYNode),
    /// A reference to another node: `{ "linkTo": "$path" }`
    Ref(String),
}

/// A node in the MOTLY output tree (has eq, properties, deleted).
#[derive(Debug, Clone, PartialEq)]
pub struct MOTLYNode {
    pub eq: Option<EqValue>,
    pub properties: Option<BTreeMap<String, MOTLYPropertyValue>>,
    pub deleted: bool,
}

impl MOTLYNode {
    pub fn new() -> Self {
        MOTLYNode {
            eq: None,
            properties: None,
            deleted: false,
        }
    }

    pub fn with_eq(eq: EqValue) -> Self {
        MOTLYNode {
            eq: Some(eq),
            properties: None,
            deleted: false,
        }
    }

    pub fn deleted() -> Self {
        MOTLYNode {
            eq: None,
            properties: None,
            deleted: true,
        }
    }

    /// Get or create the properties map.
    pub fn get_or_create_properties(&mut self) -> &mut BTreeMap<String, MOTLYPropertyValue> {
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

    /// Check if this node's eq is an env reference.
    pub fn is_env_ref(&self) -> bool {
        matches!(&self.eq, Some(EqValue::EnvRef(_)))
    }
}

impl MOTLYPropertyValue {
    /// Create a new empty node property value.
    pub fn new_node() -> Self {
        MOTLYPropertyValue::Node(MOTLYNode::new())
    }

    /// Check if this property value is a link reference.
    pub fn is_ref(&self) -> bool {
        matches!(self, MOTLYPropertyValue::Ref(_))
    }

    /// Get a reference to the inner node, if this is a Node variant.
    pub fn as_node(&self) -> Option<&MOTLYNode> {
        match self {
            MOTLYPropertyValue::Node(n) => Some(n),
            MOTLYPropertyValue::Ref(_) => None,
        }
    }

    /// Get a mutable reference to the inner node, if this is a Node variant.
    pub fn as_node_mut(&mut self) -> Option<&mut MOTLYNode> {
        match self {
            MOTLYPropertyValue::Node(n) => Some(n),
            MOTLYPropertyValue::Ref(_) => None,
        }
    }

    /// Convert a Ref to an empty Node (for intermediate path traversal).
    /// If already a Node, does nothing. Returns a mutable reference to the inner node.
    pub fn ensure_node(&mut self) -> &mut MOTLYNode {
        if matches!(self, MOTLYPropertyValue::Ref(_)) {
            *self = MOTLYPropertyValue::Node(MOTLYNode::new());
        }
        match self {
            MOTLYPropertyValue::Node(n) => n,
            _ => unreachable!(),
        }
    }
}

impl Default for MOTLYNode {
    fn default() -> Self {
        Self::new()
    }
}
