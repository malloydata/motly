from __future__ import annotations
from abc import ABC, abstractmethod
import datetime
from typing import Union, Optional, Iterator, Tuple, List, Any

# A type representing path segments for get()
PathSegment = Union[str, int]

class CallableIterator:
    """A wrapper that allows a collection to be accessed both as an iterable
    (e.g., `list(obj)`) and as a method call with optional path navigation
    (e.g., `obj(*path)`).
    """
    def __init__(self, iter_func, call_func=None):
        self._iter_func = iter_func
        self._call_func = call_func or iter_func

    def __iter__(self):
        return self._iter_func()

    def __call__(self, *args, **kwargs):
        return self._call_func(*args, **kwargs)

    def __repr__(self):
        return repr(list(self._iter_func()))

class MotFactory(ABC):
    """Factory for creating custom Mot instances.
    
    Implementations can control what subclasses of Mot are instantiated
    during parsed tree conversion.
    """
    @abstractmethod
    def create_mot(self, value: Optional[dict], properties: dict[str, Any]) -> Mot:
        """Create a resolved Mot value node."""
        pass

    @abstractmethod
    def create_ref_mot(self, ref: dict, target: Mot) -> Mot:
        """Create a reference Mot node delegating to target."""
        pass

    @property
    @abstractmethod
    def undefined_mot(self) -> Mot:
        """The singleton representing a missing/nonexistent node."""
        pass

class Mot(ABC):
    """Resolved, read-only view of a MOTLY node.
    
    Every Mot node represents a configuration element that can have both a
    value slot (scalar or array) and properties (nested config map).
    """

    @property
    @abstractmethod
    def exists(self) -> bool:
        """True for any real node (including flags). False only for Undefined."""
        pass

    @property
    @abstractmethod
    def is_ref(self) -> bool:
        """True if this Mot is a reference delegating reads to a target."""
        pass

    @abstractmethod
    def _text(self) -> Optional[str]:
        pass

    @abstractmethod
    def _numeric(self) -> Optional[Union[int, float]]:
        pass

    @abstractmethod
    def _integer(self) -> Optional[int]:
        pass

    @abstractmethod
    def _boolean(self) -> Optional[bool]:
        pass

    @abstractmethod
    def _date(self) -> Optional[Union[datetime.datetime, datetime.date]]:
        pass

    @abstractmethod
    def _values(self) -> Optional[List[Mot]]:
        pass

    @abstractmethod
    def _value_type(self) -> Optional[str]:
        pass

    @abstractmethod
    def get(self, *path: PathSegment) -> Mot:
        """Navigate properties and array indices. Returns Mot. Never None."""
        pass

    @abstractmethod
    def _keys(self) -> Iterator[str]:
        pass

    @abstractmethod
    def _items(self) -> Iterator[Tuple[str, Mot]]:
        pass

    # --- Properties supporting both Property and Method access (Dunder-like) ---

    @property
    def keys(self) -> CallableIterator:
        """Iterator of property names.
        
        Can be accessed as a property (e.g. `list(mot.keys)`) or called as a method (e.g. `mot.keys()`).
        """
        return CallableIterator(lambda: self._keys())

    @property
    def items(self) -> CallableIterator:
        """Iterator of (name, child_mot) entries.
        
        Can be accessed as a property (e.g. `dict(mot.items)`) or called as a method (e.g. `mot.items()`).
        """
        return CallableIterator(lambda: self._items())

    @property
    def values(self) -> CallableIterator:
        """Access array elements or property values.
        
        - If accessed as a property (e.g. `list(mot.values)`), returns property values or array elements.
        - If called with a path (e.g. `mot.values("ports")`), returns array elements at path.
        - If called without arguments (e.g. `mot.values()`), returns:
          - array elements if this node is an array.
          - property values if this node has nested properties.
        """
        def _get_values_iter():
            if self._value_type() == "array":
                return iter(self._values() or [])
            return (v for _, v in self.items())

        def _get_values_call(*path: PathSegment):
            if path:
                return self.get(*path)._values()
            if self._value_type() == "array":
                return self._values()
            return _get_values_iter()

        return CallableIterator(_get_values_iter, _get_values_call)

    # --- Concrete Accessors with Path Navigation ---

    def text(self, *path: PathSegment) -> Optional[str]:
        """Get string value at path, or None."""
        return self.get(*path)._text()

    def numeric(self, *path: PathSegment) -> Optional[Union[int, float]]:
        """Get numeric value (int or float) at path, or None."""
        return self.get(*path)._numeric()

    def integer(self, *path: PathSegment) -> Optional[int]:
        """Get integer value at path, or None."""
        return self.get(*path)._integer()

    def boolean(self, *path: PathSegment) -> Optional[bool]:
        """Get boolean value at path, or None."""
        return self.get(*path)._boolean()

    def date(self, *path: PathSegment) -> Optional[Union[datetime.datetime, datetime.date]]:
        """Get datetime/date value at path, or None."""
        return self.get(*path)._date()

    def value_type(self, *path: PathSegment) -> Optional[str]:
        """Get type of the value slot at path ('string', 'number', 'boolean', 'date', 'array'), or None."""
        return self.get(*path)._value_type()

    def array_elements(self, *path: PathSegment) -> Optional[List[Mot]]:
        """Get list of child Mot elements from an array value at path, or None."""
        return self.get(*path)._values()

    def elements(self, *path: PathSegment) -> Optional[List[Mot]]:
        """Alias for array_elements."""
        return self.array_elements(*path)

    def texts(self, *path: PathSegment) -> Optional[List[str]]:
        """Get list of string elements from array value, or None."""
        vals = self.get(*path)._values()
        if vals is None:
            return None
        res = []
        for m in vals:
            t = m._text()
            if t is None:
                return None
            res.append(t)
        return res

    def numerics(self, *path: PathSegment) -> Optional[List[Union[int, float]]]:
        """Get list of numeric elements from array value, or None."""
        vals = self.get(*path)._values()
        if vals is None:
            return None
        res = []
        for m in vals:
            n = m._numeric()
            if n is None:
                return None
            res.append(n)
        return res

    def integers(self, *path: PathSegment) -> Optional[List[int]]:
        """Get list of integer elements from array value, or None."""
        vals = self.get(*path)._values()
        if vals is None:
            return None
        res = []
        for m in vals:
            i = m._integer()
            if i is None:
                return None
            res.append(i)
        return res

    def booleans(self, *path: PathSegment) -> Optional[List[bool]]:
        """Get list of boolean elements from array value, or None."""
        vals = self.get(*path)._values()
        if vals is None:
            return None
        res = []
        for m in vals:
            b = m._boolean()
            if b is None:
                return None
            res.append(b)
        return res

    def dates(self, *path: PathSegment) -> Optional[List[Union[datetime.datetime, datetime.date]]]:
        """Get list of date elements from array value, or None."""
        vals = self.get(*path)._values()
        if vals is None:
            return None
        res = []
        for m in vals:
            d = m._date()
            if d is None:
                return None
            res.append(d)
        return res

    def has(self, *path: PathSegment) -> bool:
        """True if the path exists."""
        return self.get(*path).exists

    def to_native(self) -> Any:
        """Recursively convert this Mot view into native Python dictionaries, lists, and scalars."""
        if not self.exists:
            return None
        
        # Check properties first
        keys = list(self.keys)
        if keys:
            return {k: self.get(k).to_native() for k in keys}
            
        t = self.value_type()
        if t == "array":
            elems = self.array_elements()
            return [m.to_native() for m in elems] if elems is not None else []
        elif t == "string":
            return self._text()
        elif t == "number":
            return self._numeric()
        elif t == "boolean":
            return self._boolean()
        elif t == "date":
            return self._date()
        return None

    # --- Dunders ---

    def __getitem__(self, key: Union[PathSegment, Tuple[PathSegment, ...]]) -> Mot:
        if isinstance(key, tuple):
            return self.get(*key)
        return self.get(key)

    def __bool__(self) -> bool:
        return self.exists

    def __contains__(self, key: str) -> bool:
        return self.has(key)

    def __iter__(self) -> Iterator[str]:
        return self.keys()

    def __len__(self) -> int:
        return sum(1 for _ in self.keys)

    def __repr__(self) -> str:
        if not self.exists:
            return "MotUndefined()"
        val_type = self._value_type()
        if val_type == "array":
            vals = self._values()
            val_repr = f"[{len(vals)} items]" if vals is not None else "[]"
        elif val_type == "string":
            val_repr = repr(self._text())
        elif val_type == "number":
            val_repr = repr(self._numeric())
        elif val_type == "boolean":
            val_repr = repr(self._boolean())
        elif val_type == "date":
            val_repr = repr(self._date())
        else:
            val_repr = "none"
        
        keys_list = list(self.keys)
        props_repr = f"{{{', '.join(keys_list)}}}" if keys_list else "{}"
        return f"Mot(value={val_repr}, properties={props_repr})"

class MotValue(Mot):
    """A concrete Mot node with resolved value and/or properties."""
    def __init__(self, value: Optional[dict], properties: dict[str, Any], exists: bool = True):
        self._val = value
        self._props = properties
        self._exists = exists

    @property
    def exists(self) -> bool:
        return self._exists

    @property
    def is_ref(self) -> bool:
        return False

    def _text(self) -> Optional[str]:
        return self._val["value"] if self._val and self._val["type"] == "string" else None

    def _numeric(self) -> Optional[Union[int, float]]:
        if self._val and self._val["type"] == "number":
            v = self._val["value"]
            return int(v) if isinstance(v, float) and v.is_integer() else v
        return None

    def _integer(self) -> Optional[int]:
        if self._val and self._val["type"] == "number":
            v = self._val["value"]
            if isinstance(v, (int, float)) and (isinstance(v, int) or v.is_integer()):
                return int(v)
        return None

    def _boolean(self) -> Optional[bool]:
        return self._val["value"] if self._val and self._val["type"] == "boolean" else None

    def _date(self) -> Optional[Union[datetime.datetime, datetime.date]]:
        return self._val["value"] if self._val and self._val["type"] == "date" else None

    def _values(self) -> Optional[List[Mot]]:
        return self._val["value"] if self._val and self._val["type"] == "array" else None

    def _value_type(self) -> Optional[str]:
        return self._val["type"] if self._val else None

    def _keys(self) -> Iterator[str]:
        return iter(self._props.keys())

    def _items(self) -> Iterator[Tuple[str, Mot]]:
        return iter(self._props.items())

    def get(self, *path: PathSegment) -> Mot:
        current = self
        for seg in path:
            if isinstance(seg, int):
                arr = current.array_elements()
                if arr is None or seg < 0 or seg >= len(arr):
                    return _undefined_mot
                current = arr[seg]
            elif current is self:
                current = self._props.get(seg, _undefined_mot)
            else:
                current = current.get(seg)
            if not current.exists:
                return _undefined_mot
        return current

class MotRef(Mot):
    """A reference node that delegates all reads to a resolved target."""
    def __init__(self, ref_data: dict, target: Mot):
        self._ref_data = ref_data
        self._target = target

    @property
    def exists(self) -> bool:
        return True

    @property
    def is_ref(self) -> bool:
        return True

    def _text(self) -> Optional[str]: return self._target._text()
    def _numeric(self) -> Optional[Union[int, float]]: return self._target._numeric()
    def _integer(self) -> Optional[int]: return self._target._integer()
    def _boolean(self) -> Optional[bool]: return self._target._boolean()
    def _date(self) -> Optional[Union[datetime.datetime, datetime.date]]: return self._target._date()
    def _values(self) -> Optional[List[Mot]]: return self._target._values()
    def _value_type(self) -> Optional[str]: return self._target._value_type()

    def _keys(self) -> Iterator[str]: return self._target._keys()
    def _items(self) -> Iterator[Tuple[str, Mot]]: return self._target._items()

    def get(self, *path: PathSegment) -> Mot:
        return self._target.get(*path)

class MotUndefined(Mot):
    """The singleton representing a missing/nonexistent node."""
    @property
    def exists(self) -> bool: return False
    @property
    def is_ref(self) -> bool: return False

    def _text(self) -> Optional[str]: return None
    def _numeric(self) -> Optional[Union[int, float]]: return None
    def _integer(self) -> Optional[int]: return None
    def _boolean(self) -> Optional[bool]: return None
    def _date(self) -> Optional[Union[datetime.datetime, datetime.date]]: return None
    def _values(self) -> Optional[List[Mot]]: return None
    def _value_type(self) -> Optional[str]: return None

    def _keys(self) -> Iterator[str]: return iter(())
    def _items(self) -> Iterator[Tuple[str, Mot]]: return iter(())

    def get(self, *path: PathSegment) -> Mot: return self

_undefined_mot = MotUndefined()

# --- Helper functions for building Mot hierarchy ---

def parse_date(val: str) -> Union[datetime.datetime, datetime.date, str]:
    if val.endswith("Z"):
        val = val[:-1] + "+00:00"
    try:
        return datetime.datetime.fromisoformat(val)
    except Exception:
        try:
            return datetime.date.fromisoformat(val)
        except Exception:
            return val

def is_ref(pv: Any) -> bool:
    return isinstance(pv, dict) and "linkTo" in pv and "linkUps" in pv

def is_env_ref(eq: Any) -> bool:
    return isinstance(eq, dict) and "env" in eq

def navigate_ref(
    ref: dict,
    root: dict,
    ancestors: list,
    visiting: set
) -> Optional[dict]:
    link_ups = ref.get("linkUps", 0)
    link_to = ref.get("linkTo", [])
    
    if link_ups == 0:
        start = root
        start_ancestors = []
    else:
        idx = len(ancestors) - link_ups
        if idx < 0 or idx >= len(ancestors):
            return None
        start = ancestors[idx]
        start_ancestors = ancestors[:idx]

    current = start
    nav_ancestors = start_ancestors
    parent = start

    i = 0
    while i < len(link_to):
        if is_ref(current):
            ref_hash = id(current)
            if ref_hash in visiting:
                return None
            visiting.add(ref_hash)
            resolved = navigate_ref(current, root, nav_ancestors, visiting)
            visiting.remove(ref_hash)
            if not resolved:
                return None
            current = resolved["target"]
            nav_ancestors = resolved["ancestors"]
            parent = current
            i -= 1
            i += 1
            continue
            
        node = current
        seg = link_to[i]
        
        if isinstance(seg, str):
            if not isinstance(node, dict) or "properties" not in node:
                return None
            props = node["properties"] or {}
            if seg not in props:
                return None
            if i > 0:
                nav_ancestors = nav_ancestors + [parent]
            parent = node
            current = props[seg]
        else:
            if not isinstance(node, dict) or "eq" not in node:
                return None
            eq_val = node["eq"]
            if not isinstance(eq_val, list) or seg >= len(eq_val):
                return None
            if i > 0:
                nav_ancestors = nav_ancestors + [parent]
            parent = node
            current = eq_val[seg]
        i += 1

    if is_ref(current):
        ref_hash = id(current)
        if ref_hash in visiting:
            return None
        visiting.add(ref_hash)
        resolved = navigate_ref(current, root, nav_ancestors, visiting)
        visiting.remove(ref_hash)
        return resolved

    return {"target": current, "ancestors": nav_ancestors}

class DefaultMotFactory(MotFactory):
    def create_mot(self, value: Optional[dict], properties: dict[str, Any]) -> Mot:
        return MotValue(value, properties)
        
    def create_ref_mot(self, ref: dict, target: Mot) -> Mot:
        return MotRef(ref, target)
        
    @property
    def undefined_mot(self) -> Mot:
        return _undefined_mot

_default_factory = DefaultMotFactory()

def build_mot(root: dict, env: dict = None, factory: Optional[MotFactory] = None) -> Mot:
    env = env or {}
    factory = factory or _default_factory
    cache = {}

    def resolve_node(node: dict, root_node: dict, ancestors: list) -> Mot:
        if node.get("deleted"):
            return factory.undefined_mot
            
        node_id = id(node)
        if node_id in cache:
            return cache[node_id]

        properties = {}
        resolved_value = resolve_eq(node.get("eq"), root_node, ancestors, node)
        
        mot = factory.create_mot(resolved_value, properties)
        cache[node_id] = mot

        if "properties" in node and node["properties"]:
            for key, pv in node["properties"].items():
                child_mot = resolve_motly_node(pv, root_node, ancestors, node)
                if child_mot.exists:
                    properties[key] = child_mot
                    
        return mot

    def resolve_motly_node(pv: Any, root_node: dict, ancestors: list, parent_node: dict) -> Mot:
        if is_ref(pv):
            visiting = {id(pv)}
            nav = navigate_ref(pv, root_node, ancestors, visiting)
            if not nav:
                return factory.undefined_mot
            if nav["target"].get("deleted"):
                return factory.undefined_mot
            target_mot = resolve_node(nav["target"], root_node, nav["ancestors"])
            return factory.create_ref_mot(pv, target_mot)
            
        node = pv
        if not isinstance(node, dict) or node.get("deleted"):
            return factory.undefined_mot
        return resolve_node(node, root_node, ancestors + [parent_node])

    def resolve_eq(eq_val: Any, root_node: dict, ancestors: list, parent_node: dict) -> Optional[dict]:
        if eq_val is None:
            return None
            
        if isinstance(eq_val, dict) and "env" in eq_val:
            env_var = eq_val["env"]
            val = env.get(env_var)
            if val is None:
                return None
            return {"type": "string", "value": val}
            
        if isinstance(eq_val, dict) and "$date" in eq_val:
            dt = parse_date(eq_val["$date"])
            return {"type": "date", "value": dt}

        if isinstance(eq_val, list):
            arr_ancestors = ancestors + [parent_node]
            elements = []
            for elem in eq_val:
                elements.append(resolve_motly_node(elem, root_node, arr_ancestors, parent_node))
            return {"type": "array", "value": elements}

        if isinstance(eq_val, bool):
            return {"type": "boolean", "value": eq_val}
        if isinstance(eq_val, (int, float)):
            return {"type": "number", "value": eq_val}
        if isinstance(eq_val, str):
            return {"type": "string", "value": eq_val}
            
        return None

    # Initial call: root is in its own ancestor chain
    return resolve_node(root, root, [root])
