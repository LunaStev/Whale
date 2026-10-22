# SPDX-License-Identifier: MPL-2.0
import subprocess
import unittest
from unittest.mock import patch
import check_object_oracle as oracle


class WindowsEnvironmentTests(unittest.TestCase):
    def test_developer_environment_replaces_path_case_insensitively(self):
        for arch, expected in [('amd64', 'amd64'), ('aarch64', 'arm64')]:
            with (patch.dict(oracle.os.environ, {'PATH': 'old', 'PROGRAMFILES(X86)': 'C:/Tools'}, clear=True),
                  patch.object(oracle.Path, 'is_file', return_value=True),
                  patch.object(oracle.subprocess, 'check_output', return_value='C:/Program Files/VS\n'),
                  patch.object(oracle.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'Path=C:/compiler\nLIB=C:/sdk\n')) as run):
                result = oracle.windows_environment(arch)
            self.assertEqual(result['PATH'], 'C:/compiler')
            self.assertNotIn('Path', result)
            self.assertEqual(result['LIB'], 'C:/sdk')
            command = run.call_args.args[0]
            self.assertIsInstance(command, str)
            payload = command.split('/c ', 1)[1]
            self.assertEqual(payload[0], '"')
            self.assertEqual(payload[-1], '"')
            payload = payload[1:-1]  # cmd /s removes this outer pair only.
            setup = oracle.Path('C:/Program Files/VS') / 'VC/Auxiliary/Build/vcvarsall.bat'
            self.assertEqual(payload, f'call "{setup}" {expected} >nul && set')
            self.assertNotIn('\\"', payload)

    def test_setup_failure_preserves_stderr_without_printing_environment(self):
        with (patch.object(oracle.Path, 'is_file', return_value=True),
              patch.object(oracle.subprocess, 'check_output', return_value='C:/VS\n'),
              patch.object(oracle.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, '', 'bad quotation'))):
            with self.assertRaisesRegex(RuntimeError, 'bad quotation'):
                oracle.windows_environment('amd64')

    def test_missing_toolchain_is_a_clear_failure(self):
        with patch.object(oracle.Path, 'is_file', return_value=False):
            with self.assertRaisesRegex(RuntimeError, 'Visual Studio'):
                oracle.windows_environment('amd64')


if __name__ == '__main__':
    unittest.main()
