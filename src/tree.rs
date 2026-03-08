use crate::error::Position;
use std::collections::BTreeMap;

/// A source location attached to a node, relative to a specific parse() call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MOTLYLocation {
    /// Which parse() call produced this node (0-based, auto-incrementing per session).
    pub parse_id: u32,
    /// Start of the defining region.
    pub begin: Position,
    /// End of the defining region (exclusive).
    pub end: Position,
}

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
#[derive(Debug, Clone, PartialEq)]
pub enum EqValue {
    Scalar(Scalar),
    Array(Vec<MOTLYNode>),
    /// An environment variable reference: `{ "env": "NAME" }`
    EnvRef(String),
}

/// A segment in a reference path: a property name or an array index.
#[derive(Debug, Clone, PartialEq)]
pub enum RefSegment {
    Name(String),
    Index(usize),
}

/// What a property or array element leads to: either a data node or a link reference.
///
/// This is the union type that appears everywhere in the tree: as property values,
/// array elements, and at any position where a node might be a reference instead.
#[derive(Debug, Clone, PartialEq)]
pub enum MOTLYNode {
    Data(MOTLYDataNode),
    /// A structured reference to another node.
    Ref {
        link_to: Vec<RefSegment>,
        link_ups: usize,
    },
}

/// A concrete node in the MOTLY output tree (has eq, properties, deleted).
#[derive(Debug, Clone, PartialEq)]
pub struct MOTLYDataNode {
    pub eq: Option<EqValue>,
    pub properties: Option<BTreeMap<String, MOTLYNode>>,
    pub deleted: bool,
    /// Source location of this node's first appearance.
    pub location: Option<MOTLYLocation>,
}

impl MOTLYDataNode {
    pub fn new() -> Self {
        MOTLYDataNode {
            eq: None,
            properties: None,
            deleted: false,
            location: None,
        }
    }

    pub fn with_eq(eq: EqValue) -> Self {
        MOTLYDataNode {
            eq: Some(eq),
            properties: None,
            deleted: false,
            location: None,
        }
    }

    pub fn deleted() -> Self {
        MOTLYDataNode {
            eq: None,
            properties: None,
            deleted: true,
            location: None,
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

    /// Check if this node's eq is an env reference.
    pub fn is_env_ref(&self) -> bool {
        matches!(&self.eq, Some(EqValue::EnvRef(_)))
    }
}

impl MOTLYNode {
    /// Create a new empty data node wrapped in MOTLYNode::Data.
    pub fn new_data() -> Self {
        MOTLYNode::Data(MOTLYDataNode::new())
    }

    /// Check if this is a link reference.
    pub fn is_ref(&self) -> bool {
        matches!(self, MOTLYNode::Ref { .. })
    }

    /// Get a reference to the inner data node, if this is a Data variant.
    pub fn as_data_node(&self) -> Option<&MOTLYDataNode> {
        match self {
            MOTLYNode::Data(n) => Some(n),
            MOTLYNode::Ref { .. } => None,
        }
    }

    /// Get a mutable reference to the inner data node, if this is a Data variant.
    pub fn as_data_node_mut(&mut self) -> Option<&mut MOTLYDataNode> {
        match self {
            MOTLYNode::Data(n) => Some(n),
            MOTLYNode::Ref { .. } => None,
        }
    }

    /// Convert a Ref to an empty Data node (for intermediate path traversal).
    /// If already a Data node, does nothing. Returns a mutable reference to the inner data node.
    pub fn ensure_data_node(&mut self) -> &mut MOTLYDataNode {
        if matches!(self, MOTLYNode::Ref { .. }) {
            *self = MOTLYNode::Data(MOTLYDataNode::new());
        }
        match self {
            MOTLYNode::Data(n) => n,
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

impl Default for MOTLYDataNode {
    fn default() -> Self {
        Self::new()
    }
}
