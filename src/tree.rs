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

/// A segment in a reference path: a property name or an array index.
#[derive(Debug, Clone, PartialEq)]
pub enum RefSegment {
    Name(String),
    Index(usize),
}

/// What a property or array element leads to: either a node or a link reference.
///
/// A `Ref` means "this IS that other node" — no own value, no own properties.
/// A `Node` is a full node with optional eq, properties, and deleted flag.
#[derive(Debug, Clone, PartialEq)]
pub enum MOTLYPropertyValue {
    Node(MOTLYNode),
    /// A structured reference to another node.
    Ref {
        link_to: Vec<RefSegment>,
        link_ups: usize,
    },
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
        matches!(self, MOTLYPropertyValue::Ref { .. })
    }

    /// Get a reference to the inner node, if this is a Node variant.
    pub fn as_node(&self) -> Option<&MOTLYNode> {
        match self {
            MOTLYPropertyValue::Node(n) => Some(n),
            MOTLYPropertyValue::Ref { .. } => None,
        }
    }

    /// Get a mutable reference to the inner node, if this is a Node variant.
    pub fn as_node_mut(&mut self) -> Option<&mut MOTLYNode> {
        match self {
            MOTLYPropertyValue::Node(n) => Some(n),
            MOTLYPropertyValue::Ref { .. } => None,
        }
    }

    /// Convert a Ref to an empty Node (for intermediate path traversal).
    /// If already a Node, does nothing. Returns a mutable reference to the inner node.
    pub fn ensure_node(&mut self) -> &mut MOTLYNode {
        if matches!(self, MOTLYPropertyValue::Ref { .. }) {
            *self = MOTLYPropertyValue::Node(MOTLYNode::new());
        }
        match self {
            MOTLYPropertyValue::Node(n) => n,
            _ => unreachable!(),
        }
    }
}

/// Format a RefSegment slice for display: `$^^name[0].sub`
pub fn format_ref_display(ups: usize, segments: &[RefSegment]) -> String {
    let mut s = String::from("$");
    for _ in 0..ups {
        s.push('^');
    }
    let mut first = true;
    for seg in segments {
        match seg {
            RefSegment::Name(name) => {
                if !first {
                    s.push('.');
                }
                s.push_str(name);
                first = false;
            }
            RefSegment::Index(idx) => {
                s.push('[');
                s.push_str(&idx.to_string());
                s.push(']');
            }
        }
    }
    s
}

impl Default for MOTLYNode {
    fn default() -> Self {
        Self::new()
    }
}
