import unittest
import datetime
from motly import (
    MOTLYSession,
    MOTLYSchema,
    MOTLYError,
    Mot,
    MotValue,
    MotRef,
    MotUndefined,
)

class TestMotly(unittest.TestCase):
    def test_basic_parse_and_mot(self):
        s = MOTLYSession()
        res = s.parse("name = hello\nport = 8080\nssl = @true")
        self.assertEqual(res["errors"], [])
        
        result = s.finish()
        self.assertEqual(result.errors, [])
        
        mot = result.get_mot()
        self.assertTrue(mot.exists)
        self.assertEqual(mot.text("name"), "hello")
        self.assertEqual(mot.integer("port"), 8080)
        self.assertEqual(mot.numeric("port"), 8080)
        self.assertEqual(mot.boolean("ssl"), True)
        
        # Test safe traversal / Undefined Mot
        undef = mot.get("nonexistent")
        self.assertFalse(undef.exists)
        self.assertEqual(undef.text(), None)
        self.assertEqual(undef.get("nested", "keys").exists, False)
        
        # Test bracket traversal
        self.assertEqual(mot["port"].integer(), 8080)
        self.assertEqual(mot["nonexistent"]["nested"].exists, False)

    def test_dunders(self):
        s = MOTLYSession()
        s.parse("server { host = localhost  ports = [80, 443] }")
        mot = s.finish().get_mot()
        
        # __contains__ (in)
        self.assertTrue("server" in mot)
        self.assertFalse("client" in mot)
        self.assertTrue("host" in mot["server"])
        
        # __iter__ / keys()
        self.assertEqual(list(mot), ["server"])
        self.assertEqual(list(mot["server"].keys), ["host", "ports"])
        
        # __len__
        self.assertEqual(len(mot["server"]), 2)
        
        # items()
        entries = dict(mot["server"].items)
        self.assertTrue("host" in entries)
        self.assertEqual(entries["host"].text(), "localhost")

    def test_arrays(self):
        s = MOTLYSession()
        s.parse("hosts = [a, b, c]\nports = [80, 443]")
        mot = s.finish().get_mot()
        
        self.assertEqual(mot.texts("hosts"), ["a", "b", "c"])
        self.assertEqual(mot.integers("ports"), [80, 443])
        self.assertEqual(mot.numerics("ports"), [80, 443])
        
        # Test indexing via get()
        self.assertEqual(mot.text("hosts", 1), "b")
        self.assertEqual(mot.integer("ports", 0), 80)

    def test_env_refs(self):
        s = MOTLYSession()
        s.parse("path = @env.TEST_PATH")
        mot = s.finish().get_mot(env={"TEST_PATH": "/usr/bin"})
        self.assertEqual(mot.text("path"), "/usr/bin")

    def test_references(self):
        s = MOTLYSession()
        s.parse("host = localhost\nserver { port = 80  addr = $^.host }")
        mot = s.finish().get_mot()
        self.assertEqual(mot.text("server", "addr"), "localhost")
        self.assertTrue(mot.get("server", "addr").is_ref)

    def test_schema_validation(self):
        schema_src = "REQUIRED { name = string  port { VALUE = integer { MIN = 1 } } }"
        schema, errors = MOTLYSchema.parse(schema_src)
        self.assertEqual(errors, [])
        
        # Valid data
        valid_tree = {
            "properties": {
                "name": {"eq": "test"},
                "port": {"eq": 80}
            }
        }
        validation_errors = schema.validate(valid_tree)
        self.assertEqual(validation_errors, [])
        
        # Invalid data (missing name, port out of range)
        invalid_tree = {
            "properties": {
                "port": {"eq": 0}
            }
        }
        validation_errors = schema.validate(invalid_tree)
        self.assertEqual(len(validation_errors), 2)
        codes = {e.code for e in validation_errors}
        self.assertEqual(codes, {"missing-required", "out-of-range"})

    def test_context_manager(self):
        with MOTLYSession() as s:
            res = s.parse("a = 1")
            self.assertEqual(res["errors"], [])
            mot = s.finish().get_mot()
            self.assertEqual(mot.integer("a"), 1)
        self.assertTrue(s.disposed)
        with self.assertRaises(RuntimeError):
            s.parse("b = 2")

    def test_tuple_traversal(self):
        s = MOTLYSession()
        s.parse("a { b { c = hello } }")
        mot = s.finish().get_mot()
        self.assertEqual(mot["a", "b", "c"].text(), "hello")
        self.assertFalse(mot["a", "b", "nonexistent"].exists)

    def test_to_native(self):
        s = MOTLYSession()
        s.parse("name = test\nnums = [1, 2, 3]\nconfig { debug = @true }")
        mot = s.finish().get_mot()
        native = mot.to_native()
        self.assertEqual(native, {
            "name": "test",
            "nums": [1, 2, 3],
            "config": {"debug": True}
        })

    def test_falsy_repr(self):
        s = MOTLYSession()
        s.parse("zero = 0\nflag = @false")
        mot = s.finish().get_mot()
        self.assertIn("value=0", repr(mot["zero"]))
        self.assertIn("value=False", repr(mot["flag"]))

    def test_reset_lifecycle(self):
        s = MOTLYSession()
        s.parse("a = 1")
        res1 = s.finish()
        self.assertEqual(res1.get_mot().integer("a"), 1)
        
        # Reset and parse again
        s.reset()
        s.parse("b = 2")
        res2 = s.finish()
        self.assertFalse(res2.get_mot().has("a"))
        self.assertEqual(res2.get_mot().integer("b"), 2)

    def test_custom_factory(self):
        from motly import MotFactory, MotValue, MotRef, MotUndefined
        
        class TrackingFactory(MotFactory):
            def __init__(self):
                self.created_count = 0
            def create_mot(self, value, properties):
                self.created_count += 1
                return MotValue(value, properties)
            def create_ref_mot(self, ref, target):
                return MotRef(ref, target)
            @property
            def undefined_mot(self):
                return MotUndefined()

        factory = TrackingFactory()
        s = MOTLYSession()
        s.parse("a = 1\nb = 2")
        mot = s.finish().get_mot(factory=factory)
        self.assertEqual(mot.integer("a"), 1)
        self.assertGreater(factory.created_count, 0)

    def test_thread_safety(self):
        import threading
        schema_src = "REQUIRED { name = string }"
        schema, _ = MOTLYSchema.parse(schema_src)
        
        errors_list = []
        def worker(name_val):
            tree = {"properties": {"name": {"eq": name_val}}}
            errs = schema.validate(tree)
            errors_list.append(errs)

        threads = [threading.Thread(target=worker, args=(f"thread-{i}",)) for i in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
            
        self.assertEqual(len(errors_list), 10)
        for errs in errors_list:
            self.assertEqual(errs, [])

