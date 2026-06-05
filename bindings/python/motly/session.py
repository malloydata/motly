from __future__ import annotations
import json
import copy
import threading
from typing import Optional, Union
from .ffi import get_lib, decode_and_free_string
from .mot import build_mot, Mot, MotFactory

class MOTLYError(Exception):
    """An error encountered during MOTLY parsing or execution."""
    def __init__(self, code: str, message: str, begin: dict, end: dict):
        self.code = code
        self.message = message
        self.begin = begin
        self.end = end
        super().__init__(f"[{code}] {message}")
        
    def __repr__(self):
        return f"MOTLYError(code={self.code!r}, message={self.message!r})"

class MOTLYSchemaError:
    """An error found during schema validation."""
    def __init__(self, code: str, message: str, path: list[str], location: dict = None):
        self.code = code
        self.message = message
        self.path = path
        self.location = location
        
    def __repr__(self):
        return f"MOTLYSchemaError(code={self.code!r}, path={self.path!r}, message={self.message!r})"

class MOTLYValidationError:
    """An error found during reference validation."""
    def __init__(self, code: str, message: str, path: list[str], location: dict = None):
        self.code = code
        self.message = message
        self.path = path
        self.location = location
        
    def __repr__(self):
        return f"MOTLYValidationError(code={self.code!r}, path={self.path!r}, message={self.message!r})"

class MOTLYSession:
    """A write-only MOTLY parsing session that accumulates input.
    
    This class manages a native session via Rust FFI. Supports context manager syntax
    for deterministic native memory release.
    """
    def __init__(self, options: dict = None):
        self._lib = get_lib()
        self.options = options or {}
        flags = 0
        # support both camelCase and snake_case for disableReferences
        if self.options.get("disableReferences") or self.options.get("disable_references"):
            flags |= 1
        self.session_id = self._lib.wasm_session_new_with_options(flags)
        self.finished = False
        self.disposed = False

    def __enter__(self) -> MOTLYSession:
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.dispose()

    def parse(self, source: str) -> dict:
        """Parse source and accumulate statements. Returns syntax errors."""
        self._ensure_alive()
        if self.finished:
            raise RuntimeError("MOTLYSession is spent after finish() — create a new session")
        source_bytes = source.encode("utf-8")
        ptr = self._lib.wasm_session_parse(self.session_id, source_bytes, len(source_bytes))
        res_str = decode_and_free_string(ptr)
        res_json = json.loads(res_str)
        errors = [
            MOTLYError(
                e["code"],
                e["message"],
                e["begin"],
                e["end"]
            )
            for e in res_json["errors"]
        ]
        return {
            "parse_id": res_json["parseId"],
            "parseId": res_json["parseId"], # For backward compatibility
            "errors": errors
        }

    def parse_schema(self, source: str) -> dict:
        """Parse MOTLY source as a schema and store it in this session."""
        self._ensure_alive()
        source_bytes = source.encode("utf-8")
        ptr = self._lib.wasm_session_parse_schema(self.session_id, source_bytes, len(source_bytes))
        res_str = decode_and_free_string(ptr)
        res_json = json.loads(res_str)
        errors = [
            MOTLYError(
                e["code"],
                e["message"],
                e["begin"],
                e["end"]
            )
            for e in res_json["errors"]
        ]
        return {
            "parse_id": res_json["parseId"],
            "parseId": res_json["parseId"],
            "errors": errors
        }

    def finish(self) -> MOTLYResult:
        """Interpret all statements and return the final resolved tree and errors."""
        self._ensure_alive()
        if self.finished:
            raise RuntimeError("finish() has already been called on this session")
        self.finished = True
        ptr = self._lib.wasm_session_finish(self.session_id)
        res_str = decode_and_free_string(ptr)
        res_json = json.loads(res_str)
        errors = [
            MOTLYError(
                e["code"],
                e["message"],
                e["begin"],
                e["end"]
            )
            for e in res_json
        ]
        
        # Get raw JSON wire representation of final tree
        val_ptr = self._lib.wasm_session_get_value(self.session_id)
        val_str = decode_and_free_string(val_ptr)
        val_json = json.loads(val_str) if val_str else {}
        
        return MOTLYResult(val_json, errors)

    def reset(self):
        """Reset the session value and accumulated parsing state, allowing reuse."""
        self._ensure_alive()
        self._lib.wasm_session_reset(self.session_id)
        self.finished = False

    def validate_refs(self) -> list[MOTLYValidationError]:
        """Validate references in the session's value."""
        self._ensure_alive()
        ptr = self._lib.wasm_session_validate_refs(self.session_id)
        res_str = decode_and_free_string(ptr)
        res_json = json.loads(res_str)
        return [
            MOTLYValidationError(
                e["code"],
                e["message"],
                e["path"],
                e.get("location")
            )
            for e in res_json
        ]

    def validate_schema(self) -> list[MOTLYSchemaError]:
        """Validate the session's value against its stored schema."""
        self._ensure_alive()
        ptr = self._lib.wasm_session_validate_schema(self.session_id)
        res_str = decode_and_free_string(ptr)
        res_json = json.loads(res_str)
        return [
            MOTLYSchemaError(
                e["code"],
                e["message"],
                e["path"],
                e.get("location")
            )
            for e in res_json
        ]

    def dispose(self):
        """Release native resources held by this session."""
        if not self.disposed:
            if self._lib is not None:
                try:
                    self._lib.wasm_session_free(self.session_id)
                except (TypeError, AttributeError):
                    pass # Safely ignore errors during Python interpreter shutdown
            self.disposed = True

    def _ensure_alive(self):
        if self.disposed:
            raise RuntimeError("MOTLYSession has been disposed")

    def __del__(self):
        try:
            self.dispose()
        except Exception:
            pass

class MOTLYResult:
    """Immutable result from MOTLYSession.finish()."""
    def __init__(self, value: dict, errors: list[MOTLYError]):
        self._value = value
        self.errors = errors

    def get_value(self) -> dict:
        """Return a deep copy of the interpreted tree."""
        return copy.deepcopy(self._value)

    def get_mot(self, env: dict = None, factory: Optional[MotFactory] = None) -> Mot:
        """Return a resolved read-only Mot view of the tree."""
        return build_mot(self._value, env=env, factory=factory)

class MOTLYSchema:
    """A parsed MOTLY schema, independent of any session.
    
    This class is thread-safe for validation calls.
    """
    def __init__(self, session_id: int, session: MOTLYSession):
        self._session_id = session_id
        self._session = session
        self._lock = threading.Lock()

    @classmethod
    def parse(cls, source: str) -> tuple[MOTLYSchema, list[MOTLYError]]:
        """Parse MOTLY source as a schema."""
        session = MOTLYSession(options={"disableReferences": True})
        res = session.parse_schema(source)
        schema = cls(session.session_id, session)
        return schema, res["errors"]

    def validate(self, tree: dict) -> list[MOTLYSchemaError]:
        """Validate a MOTLY data tree against this schema."""
        class WireEncoder(json.JSONEncoder):
            def default(self, obj):
                import datetime
                if isinstance(obj, (datetime.datetime, datetime.date)):
                    return {"$date": obj.isoformat()}
                return super().default(obj)

        json_str = json.dumps(tree, cls=WireEncoder)
        json_bytes = json_str.encode("utf-8")
        
        with self._lock:
            success = get_lib().wasm_session_set_value(self._session_id, json_bytes, len(json_bytes))
            if not success:
                raise ValueError("Failed to load data tree into schema session")
                
            return self._session.validate_schema()
