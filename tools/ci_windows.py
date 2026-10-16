"""Private Windows process-tree ownership for the non-interactive CI runner.

The Python gate cannot launch the requested command until it belongs to our
kill-on-close Job. This avoids the assign-after-spawn descendant race. Nested
Jobs are supported on the Windows versions in Whale's native CI matrix.
https://learn.microsoft.com/windows/win32/procthread/job-objects
"""

import ctypes
from ctypes import wintypes


class Job:
    def __init__(self):
        size = ctypes.c_size_t

        class Basic(ctypes.Structure):
            _fields_ = [
                ("user_time", ctypes.c_longlong), ("job_time", ctypes.c_longlong),
                ("flags", wintypes.DWORD), ("minimum", size), ("maximum", size),
                ("active", wintypes.DWORD), ("affinity", size),
                ("priority", wintypes.DWORD), ("scheduling", wintypes.DWORD),
            ]

        class Extended(ctypes.Structure):
            _fields_ = [("basic", Basic), ("io", ctypes.c_ulonglong * 6),
                        ("process_memory", size), ("job_memory", size),
                        ("peak_process", size), ("peak_job", size)]

        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        for name, arguments, result in [
            ("CreateJobObjectW", [ctypes.c_void_p, wintypes.LPCWSTR], wintypes.HANDLE),
            ("SetInformationJobObject", [wintypes.HANDLE, ctypes.c_int,
                                         ctypes.c_void_p, wintypes.DWORD], wintypes.BOOL),
            ("AssignProcessToJobObject", [wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
            ("CloseHandle", [wintypes.HANDLE], wintypes.BOOL),
        ]:
            function = getattr(self.api, name)
            function.argtypes = arguments
            function.restype = result
        self.handle = self.api.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        limits = Extended()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self.api.SetInformationJobObject(self.handle, 9, ctypes.byref(limits),
                                                ctypes.sizeof(limits)):
            error = ctypes.WinError(ctypes.get_last_error())
            self.close()
            raise error

    def assign(self, process):
        if not self.api.AssignProcessToJobObject(self.handle, int(process._handle)):
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self):
        if self.handle:
            handle, self.handle = self.handle, None
            if not self.api.CloseHandle(handle):
                raise ctypes.WinError(ctypes.get_last_error())
