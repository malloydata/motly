# MOTLY Python API Bindings

This package provides high-performance Python bindings for the **MOTLY** configuration language, wrapping the native Rust engine. 

---

## Installation & Build

1. Build the Rust core engine shared library in release mode:
   ```sh
   cargo build --release
   ```
2. Install the Python package in editable mode (or via standard install):
   ```sh
   pip install -e .
   ```

---

## Quick Start

```python
from motly import MOTLYSession

# 1. Parse and compile MOTLY source
with MOTLYSession() as session:
    session.parse("server { host = localhost  port = 8080 }")
    session.parse("database { url = db://localhost  max_conn = 10 }")
    
    # Evaluate and resolve the AST
    result = session.finish()

# 2. Extract resolved values using the Mot tree view
mot = result.get_mot()

print(mot["server", "host"].text())       # Output: "localhost"
print(mot["database", "max_conn"].integer())  # Output: 10
```

---

## API Reference

### `MOTLYSession`

A stateful, write-only parsing session that accumulates MOTLY input statements. `MOTLYSession` implements the context manager protocol (`with` blocks) to ensure deterministic cleanup of underlying Rust memory.

#### Methods

*   **`__init__(options: dict = None)`**
    Instantiates a new session.
    *   `options`: Optional configurations. Supported options:
        *   `disable_references` (bool): Disable reference resolution.
*   **`parse(source: str) -> dict`**
    Parses a string of MOTLY statements. Returns a dictionary with the compile metadata.
    *   Returns: `{"parse_id": int, "errors": list[MOTLYError]}`
*   **`parse_schema(source: str) -> dict`**
    Parses MOTLY statements specifically to load a schema definition.
*   **`finish() -> MOTLYResult`**
    Triggers reference evaluation, topologically sorts the dependency tree, executes the four-phase interpreter on the Rust side, and returns the final immutable `MOTLYResult`.
*   **`reset()`**
    Resets the session state (clearing parser history and parsed results) so the same session ID can be reused for a new execution lifecycle.
*   **`dispose()`**
    Manually frees the native Rust session memory. Safe to call multiple times.

---

### `MOTLYResult`

The immutable evaluation result returned by `MOTLYSession.finish()`.

#### Attributes

*   **`errors`** (`list[MOTLYError]`): List of syntactic or semantic compilation errors.

#### Methods

*   **`get_value() -> dict`**
    Returns a deep copy of the underlying raw AST wire-format dictionary representation.
*   **`get_mot(env: dict = None, factory: MotFactory = None) -> Mot`**
    Returns a navigable, resolved `Mot` node representation of the configuration root.
    *   `env` (dict): Optional key-value dictionary to resolve environment variable queries (e.g. `@env.PORT`).
    *   `factory` (`MotFactory`): Optional custom factory to construct custom `Mot` subclasses.

---

### `Mot` (Abstract Base Class)

A resolved read-only hierarchical view of a configuration node. A `Mot` node behaves like both a **dictionary** (has property keys/values) and a **value** (can contain a scalar or list value).

#### Navigation & Bracket Traversal

Nodes can be navigated safely using bracket traversal or the `get` method. Missing paths return an inactive `MotUndefined` instance instead of raising a `KeyError` or returning `None`, allowing safe chaining.

```python
# Chained bracket syntax
host = mot["server"]["host"]

# Tuple multi-path syntax (Recommended)
host = mot["server", "host"]

# Safe navigation: does not raise errors even if path does not exist
nonexistent = mot["server", "nonexistent", "deep", "nested"]
assert not nonexistent.exists
assert nonexistent.text() is None
```

#### Dunder Methods & Mapping Support

*   **`if mot:` / `bool(mot)`**: Evaluates whether the node exists.
*   **`len(mot)`**: Returns the count of properties (keys) on the node.
*   **`for key in mot:`**: Iterates over the property keys.
*   **`"key" in mot`**: Membership test mapping to `has(key)`.
*   **`repr(mot)`**: Formats a detailed debugging view showing the resolved value type and properties.

#### Iteration Dual-Nature

The properties `.keys`, `.items`, and `.values` can be accessed both as **attributes** (compatible with standard lists/tuples) and called as **methods** (pythonic dictionary compatibility):

```python
# Property access style
keys_list = list(mot.keys)
entries = dict(mot.items)

# Method call style
keys_list = list(mot.keys())
entries = dict(mot.items())
```

#### Concrete Accessors

Every accessor accepts an optional `*path` to navigate hierarchy before querying:

*   **`exists`** (`bool` property): True if the node is defined and not deleted.
*   **`is_ref`** (`bool` property): True if the node delegates to another location via reference.
*   **`text(*path) -> Optional[str]`**: Resolves value as a string.
*   **`integer(*path) -> Optional[int]`**: Resolves value as an integer.
*   **`numeric(*path) -> Optional[int, float]`**: Resolves value as a float or integer.
*   **`boolean(*path) -> Optional[bool]`**: Resolves value as a boolean.
*   **`date(*path) -> Optional[datetime.datetime, datetime.date]`**: Resolves value as a date/datetime object.
*   **`value_type(*path) -> Optional[str]`**: Resolves slot type: `'string'`, `'number'`, `'boolean'`, `'date'`, or `'array'`.
*   **`array_elements(*path) -> Optional[list[Mot]]`**: Returns a list of child `Mot` elements.
*   **`to_native() -> Any`**: Recursively serializes the node and its sub-properties into pure Python native types (dicts, lists, and scalars).

---

### `MOTLYSchema`

A compiled schema that validates MOTLY configurations. Thread-safe for concurrent validation calls.

#### Methods

*   **`parse(source: str) -> tuple[MOTLYSchema, list[MOTLYError]]`** (Class Method)
    Compiles MOTLY schema syntax.
*   **`validate(tree: dict) -> list[MOTLYSchemaError]`**
    Validates a raw wire-format configuration tree against the schema. Returns a list of validation errors (e.g. `missing-required`, `out-of-range`, `type-mismatch`).

---

## Advanced Usage

### Custom Class Factories

You can pass a custom `MotFactory` to `get_mot()` to override how `Mot` instances are instantiated. This is ideal for logging, telemetry, access control, or read tracking:

```python
from motly import MotFactory, MotValue, MotRef, MotUndefined

class AccessLoggingFactory(MotFactory):
    def __init__(self):
        self.access_log = []
        
    def create_mot(self, value, properties):
        print(f"Creating node with value: {value}")
        return MotValue(value, properties)
        
    def create_ref_mot(self, ref, target):
        return MotRef(ref, target)
        
    @property
    def undefined_mot(self):
        return MotUndefined()

factory = AccessLoggingFactory()
mot = result.get_mot(factory=factory)
```
