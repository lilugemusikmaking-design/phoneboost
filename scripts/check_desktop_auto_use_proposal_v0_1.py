#!/usr/bin/env python3
"""Proposal-only wire oracles; NOT production dispatch or persistence proof."""

import json
import unittest


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result


def parse(raw):
    return json.loads(raw, object_pairs_hook=unique_object)


def params(method, raw, descriptors=0):
    value = parse(raw)
    if descriptors != 0 or type(value) is not dict:
        return False
    if method == "auto_use.get":
        return value == {}
    return (method == "auto_use.set" and set(value) == {"enabled"}
            and type(value["enabled"]) is bool)


def result(raw, setter=False):
    value = parse(raw)
    return (type(value) is dict and set(value) == {"enabled", "persistence"}
            and type(value["enabled"]) is bool
            and value["persistence"] in ("DEFAULT", "SAVED", "ERROR")
            and (value["persistence"] != "ERROR" or not value["enabled"])
            and (value["persistence"] != "DEFAULT" or value["enabled"])
            and (not setter or value["persistence"] == "SAVED"))


def config(raw):
    if len(raw.encode("utf-8")) > 256:
        return False
    value = parse(raw)
    return (type(value) is dict
            and set(value) == {"schema_version", "auto_use_enabled"}
            and type(value["schema_version"]) is int
            and value["schema_version"] == 1
            and type(value["auto_use_enabled"]) is bool)


class ProposalVectors(unittest.TestCase):
    def test_exact_requests(self):
        self.assertTrue(params("auto_use.get", "{}"))
        for enabled in (True, False):
            self.assertTrue(params("auto_use.set", json.dumps({"enabled": enabled})))

    def test_bad_set_shapes(self):
        for raw in ('{}', 'null', '[]', 'false', '{"enabled":0}',
                    '{"enabled":1}', '{"enabled":"false"}', '{"enabled":null}',
                    '{"enabled":false,"path":"/tmp/input"}',
                    '{"enabled":false,"lease_id":"fake"}'):
            with self.subTest(raw=raw):
                self.assertFalse(params("auto_use.set", raw))

    def test_no_descriptor_or_generic_method(self):
        for method, raw in (("auto_use.get", "{}"),
                            ("auto_use.set", '{"enabled":false}')):
            self.assertFalse(params(method, raw, descriptors=1))
        self.assertFalse(params("Auto_use.set", '{"enabled":false}'))
        self.assertFalse(params("controller.release", '{"enabled":false}'))
        self.assertFalse(params("auto_use.get", '{"enabled":false}'))

    def test_duplicate_keys_and_invalid_json(self):
        for raw in ('{"enabled":true,"enabled":false}',
                    '{"schema_version":1,"schema_version":2}', '{'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                parse(raw)

    def test_result_truth(self):
        for persistence in ("DEFAULT", "SAVED", "ERROR"):
            for enabled in (False, True):
                raw = json.dumps(dict(enabled=enabled, persistence=persistence))
                valid = (persistence == "SAVED" or
                         (persistence == "DEFAULT" and enabled) or
                         (persistence == "ERROR" and not enabled))
                self.assertEqual(result(raw), valid)
                self.assertEqual(result(raw, setter=True), persistence == "SAVED")
        for raw in ('{}', 'null', '{"enabled":false}',
                    '{"enabled":0,"persistence":"SAVED"}',
                    '{"enabled":true,"persistence":"READY"}',
                    '{"enabled":true,"persistence":"SAVED","ready":true}'):
            self.assertFalse(result(raw))

    def test_config_exact_and_bounded(self):
        for enabled in (False, True):
            self.assertTrue(config(json.dumps(dict(schema_version=1, auto_use_enabled=enabled))))
        for value in ({}, None, {"schema_version": True, "auto_use_enabled": False},
                      {"schema_version": 2, "auto_use_enabled": False},
                      {"schema_version": 1, "auto_use_enabled": 0},
                      {"schema_version": 1, "auto_use_enabled": False, "path": "x"}):
            self.assertFalse(config(json.dumps(value)))
        self.assertFalse(config(' ' * 256 + '{"schema_version":1,"auto_use_enabled":false}'))


if __name__ == "__main__":
    unittest.main(verbosity=2)
