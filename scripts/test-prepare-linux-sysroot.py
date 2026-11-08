#!/usr/bin/env python3
"""Offline tests for bounded Linux sysroot network retries."""

import importlib.util
import http.client
import io
from pathlib import Path
import sys
import unittest
import urllib.error
from unittest.mock import patch

path = Path(__file__).with_name("prepare_linux_sysroot.py")
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("prepare_linux_sysroot", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class DownloadRetryTests(unittest.TestCase):
    @patch.object(module.time, "sleep")
    @patch.object(module.urllib.request, "urlopen")
    def test_partial_failure_then_success(self, urlopen, sleep):
        urlopen.side_effect = [TimeoutError("timeout"), io.BytesIO(b"complete")]
        self.assertEqual(module.download("https://example.invalid/package"), b"complete")
        self.assertEqual(urlopen.call_count, 2)
        sleep.assert_called_once_with(1)
        self.assertEqual(urlopen.call_args.kwargs["timeout"], 45)

    @patch.object(module.time, "sleep")
    @patch.object(module.urllib.request, "urlopen")
    def test_transient_exhaustion_and_permanent_http_error(self, urlopen, sleep):
        urlopen.side_effect = TimeoutError("timeout")
        with self.assertRaises(TimeoutError):
            module.download("https://example.invalid/package")
        self.assertEqual(urlopen.call_count, 3)
        self.assertEqual(sleep.call_count, 2)
        urlopen.reset_mock()
        sleep.reset_mock()
        urlopen.side_effect = urllib.error.HTTPError("https://example.invalid", 403, "forbidden", None, None)
        with self.assertRaises(urllib.error.HTTPError):
            module.download("https://example.invalid/package")
        self.assertEqual(urlopen.call_count, 1)
        sleep.assert_not_called()

    @patch.object(module.time, "sleep")
    @patch.object(module.urllib.request, "urlopen")
    def test_server_error_and_partial_read_do_not_pollute_result(self, urlopen, sleep):
        response = unittest.mock.MagicMock()
        response.__enter__.return_value.read.side_effect = http.client.IncompleteRead(b"partial")
        urlopen.side_effect = [
            urllib.error.HTTPError("https://example.invalid", 503, "unavailable", None, None),
            response,
            io.BytesIO(b"complete"),
        ]
        self.assertEqual(module.download("https://example.invalid/package"), b"complete")
        self.assertEqual(urlopen.call_count, 3)
        self.assertEqual(sleep.call_count, 2)


if __name__ == "__main__":
    unittest.main()
