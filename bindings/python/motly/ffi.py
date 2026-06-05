import ctypes
import sys
from pathlib import Path

def find_library() -> str:
    this_dir = Path(__file__).resolve().parent
    workspace_root = this_dir.parents[2] # bindings/python/motly/ffi.py -> bindings/python/motly -> bindings/python -> workspace_root
    
    candidates = [
        workspace_root / "target" / "release",
        workspace_root / "target" / "debug",
        this_dir,
    ]
    
    lib_names = []
    if sys.platform == "darwin":
        lib_names = ["libmotly_rust.dylib"]
    elif sys.platform == "win32":
        lib_names = ["motly_rust.dll", "libmotly_rust.dll"]
    else:
        lib_names = ["libmotly_rust.so"]
        
    for candidate in candidates:
        for name in lib_names:
            path = candidate / name
            if path.exists():
                return str(path)
                
    # Fallback to system loader
    for name in lib_names:
        return name
            
    raise ImportError(
        "Could not find motly-rust dynamic library. "
        "Please run 'cargo build --release' in the repository root first."
    )

_lib = None

def get_lib():
    global _lib
    if _lib is None:
        lib_path = find_library()
        try:
            _lib = ctypes.CDLL(lib_path)
        except Exception as e:
            raise ImportError(
                f"Failed to load motly-rust dynamic library from '{lib_path}': {e}. "
                "Ensure the Rust library is built using 'cargo build --release'."
            )
            
        # Configure FFI signatures
        _lib.dealloc.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        _lib.dealloc.restype = None
        
        _lib.wasm_session_new_with_options.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_new_with_options.restype = ctypes.c_uint32
        
        _lib.wasm_session_parse.argtypes = [ctypes.c_uint32, ctypes.c_char_p, ctypes.c_size_t]
        _lib.wasm_session_parse.restype = ctypes.c_void_p
        
        _lib.wasm_session_finish.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_finish.restype = ctypes.c_void_p
        
        _lib.wasm_session_parse_schema.argtypes = [ctypes.c_uint32, ctypes.c_char_p, ctypes.c_size_t]
        _lib.wasm_session_parse_schema.restype = ctypes.c_void_p
        
        _lib.wasm_session_reset.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_reset.restype = None
        
        _lib.wasm_session_get_value.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_get_value.restype = ctypes.c_void_p
        
        _lib.wasm_session_validate_refs.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_validate_refs.restype = ctypes.c_void_p
        
        _lib.wasm_session_validate_schema.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_validate_schema.restype = ctypes.c_void_p
        
        _lib.wasm_session_free.argtypes = [ctypes.c_uint32]
        _lib.wasm_session_free.restype = None
        
    return _lib

def decode_and_free_string(ptr: ctypes.c_void_p) -> str:
    if not ptr:
        return ""
    char_ptr = ctypes.cast(ptr, ctypes.c_char_p)
    val_bytes = char_ptr.value
    val_str = val_bytes.decode("utf-8") if val_bytes else ""
    # Free the Rust allocated string using dealloc
    get_lib().dealloc(ptr, len(val_bytes) + 1)
    return val_str
