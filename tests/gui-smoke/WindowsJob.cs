// Windows only. No process-name enumeration: ownership starts before the first
// instruction of the suspended child, and descendants inherit the same job.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public sealed class MaxedGuiSmokeJob : IDisposable
{
    private IntPtr job;
    private bool terminated;

    public MaxedGuiSmokeJob()
    {
        job = CreateJobObjectW(IntPtr.Zero, null);
        if (job == IntPtr.Zero) throw new Win32Exception();
        var limits = new ExtendedLimits();
        limits.BasicLimitInformation.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE; no breakaway
        if (!SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimits))))
        {
            var error = new Win32Exception();
            CloseHandle(job); job = IntPtr.Zero;
            throw error;
        }
    }

    public Process Start(ProcessStartInfo info)
    {
        if (job == IntPtr.Zero) throw new ObjectDisposedException("MaxedGuiSmokeJob");
        if (terminated) throw new InvalidOperationException("Smoke job was already terminated");
        var keys = new List<string>();
        foreach (string key in info.EnvironmentVariables.Keys) keys.Add(key);
        keys.Sort(StringComparer.OrdinalIgnoreCase);
        var environment = new StringBuilder();
        foreach (string key in keys) environment.Append(key).Append('=').Append(info.EnvironmentVariables[key]).Append('\0');
        environment.Append('\0');
        IntPtr block = Marshal.StringToHGlobalUni(environment.ToString());
        var startup = new StartupInfo();
        startup.cb = Marshal.SizeOf(typeof(StartupInfo));
        ProcessInfo child = new ProcessInfo();
        Process process = null;
        bool resumed = false;
        try
        {
            var command = new StringBuilder("\"" + info.FileName + "\" " + info.Arguments);
            if (!CreateProcessW(info.FileName, command, IntPtr.Zero, IntPtr.Zero, false,
                0x00000004 | 0x00000400, block, info.WorkingDirectory, ref startup, out child))
                throw new Win32Exception(); // CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT
            if (!AssignProcessToJobObject(job, child.Process)) throw new Win32Exception();
            process = Process.GetProcessById((int)child.ProcessId);
            // Retain a process handle while the child still exists.
            IntPtr retained = process.Handle;
            if (ResumeThread(child.Thread) == UInt32.MaxValue) throw new Win32Exception();
            resumed = true;
            return process;
        }
        catch (Win32Exception error)
        {
            throw new InvalidOperationException(
                "Could not start application '" + info.FileName + "' in working directory '" +
                info.WorkingDirectory + "': " + error.Message + " (Win32 error " + error.NativeErrorCode + ")", error);
        }
        finally
        {
            if (!resumed && child.Process != IntPtr.Zero)
            {
                TerminateProcess(child.Process, 1);
                WaitForSingleObject(child.Process, 10000);
                if (process != null) process.Dispose();
            }
            if (child.Thread != IntPtr.Zero) CloseHandle(child.Thread);
            if (child.Process != IntPtr.Zero) CloseHandle(child.Process);
            Marshal.FreeHGlobal(block);
        }
    }

    public void Terminate()
    {
        if (job == IntPtr.Zero || terminated) return;
        if (!TerminateJobObject(job, 1)) throw new Win32Exception();
        var clock = Stopwatch.StartNew();
        while (true)
        {
            Accounting counts;
            if (!QueryInformationJobObject(job, 1, out counts, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero))
                throw new Win32Exception();
            if (counts.ActiveProcesses == 0) break;
            if (clock.ElapsedMilliseconds >= 10000) throw new TimeoutException("Smoke job processes did not exit within ten seconds");
            Thread.Sleep(25);
        }
        terminated = true;
    }

    internal bool ContainsProcess(IntPtr process)
    {
        if (job == IntPtr.Zero) throw new ObjectDisposedException("MaxedGuiSmokeJob");
        bool member;
        if (!IsProcessInJob(process, job, out member)) throw new Win32Exception();
        return member;
    }

    public void Dispose()
    {
        if (job == IntPtr.Zero) return;
        try { Terminate(); }
        finally { CloseHandle(job); job = IntPtr.Zero; }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimits
    {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters { public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes; }
    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimits
    {
        public BasicLimits BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct Accounting
    {
        public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public int cb;
        public string Reserved, Desktop, Title;
        public uint X, Y, XSize, YSize, XCountChars, YCountChars, FillAttribute, Flags;
        public ushort ShowWindow, Reserved2;
        public IntPtr ReservedPtr, StdInput, StdOutput, StdError;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInfo { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedLimits limits, uint length);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting accounting, uint length, IntPtr returnedLength);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processAttributes,
        IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string directory,
        ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll")]
    private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);
}

// Diagnostic-only Restart Manager use: never RmShutdown/RmRestart or any kill.
public sealed class MaxedGuiSmokeLocker
{
    public int process_id { get; set; }
    public string image_name { get; set; }
    public string application_name { get; set; }
    public string started_utc { get; set; }
    public bool? alive { get; set; }
    public bool? in_job { get; set; }
    public string diagnostic_error { get; set; }
}

public static class MaxedGuiSmokeRestartManager
{
    public static MaxedGuiSmokeLocker[] GetLockers(string[] files, MaxedGuiSmokeJob job)
    {
        if (files.Length == 0) return new MaxedGuiSmokeLocker[0];
        uint session;
        Check(RmStartSession(out session, 0, new StringBuilder(33)), "RmStartSession");
        try
        {
            Check(RmRegisterResources(session, (uint)files.Length, files, 0, IntPtr.Zero, 0, IntPtr.Zero), "RmRegisterResources");
            uint needed, count = 0, reasons;
            RmProcessInfo[] entries = null;
            // The list may grow between calls; do not mistake ERROR_MORE_DATA
            // for an empty list. Bound retries if process churn never settles.
            for (int attempt = 0; attempt < 8; attempt++)
            {
                uint result = RmGetList(session, out needed, ref count, entries, out reasons);
                if (result == 234)
                {
                    count = needed;
                    entries = new RmProcessInfo[count];
                    continue;
                }
                Check(result, "RmGetList");
                var lockers = new List<MaxedGuiSmokeLocker>();
                for (int i = 0; i < count; i++) lockers.Add(Inspect(entries[i], job));
                return lockers.ToArray();
            }
            throw new InvalidOperationException("RmGetList process list kept changing after eight attempts");
        }
        finally { Check(RmEndSession(session), "RmEndSession"); }
    }

    private static MaxedGuiSmokeLocker Inspect(RmProcessInfo entry, MaxedGuiSmokeJob job)
    {
        long started = FileTimeValue(entry.Process.StartTime);
        var locker = new MaxedGuiSmokeLocker {
            process_id = (int)entry.Process.ProcessId,
            image_name = "<unavailable>",
            application_name = entry.ApplicationName,
            started_utc = DateTime.FromFileTimeUtc(started).ToString("o")
        };
        // Minimum rights: full Process.Handle access may fail for a scanner or
        // service owned by somebody else. Never request termination access.
        IntPtr process = OpenProcess(0x00100000 | 0x1000, false, entry.Process.ProcessId);
        if (process == IntPtr.Zero)
        {
            int error = Marshal.GetLastWin32Error();
            if (error == 87) locker.alive = false; // PID exited before inspection.
            locker.diagnostic_error = "OpenProcess: " + new Win32Exception(error).Message + " (" + error + ")";
            return locker;
        }
        try
        {
            FileTime creation, exit, kernel, user;
            if (!GetProcessTimes(process, out creation, out exit, out kernel, out user)) throw new Win32Exception();
            if (FileTimeValue(creation) != started)
            {
                locker.alive = false;
                locker.diagnostic_error = "PID reused since Restart Manager snapshot; original locker exited";
                return locker;
            }
            uint wait = WaitForSingleObject(process, 0);
            if (wait == UInt32.MaxValue) throw new Win32Exception();
            locker.alive = wait == 258; // WAIT_TIMEOUT: verified original process is still alive.
            var image = new StringBuilder(32768);
            uint length = (uint)image.Capacity;
            if (QueryFullProcessImageNameW(process, 0, image, ref length))
                locker.image_name = System.IO.Path.GetFileName(image.ToString());
            else locker.diagnostic_error = "QueryFullProcessImageName: " + new Win32Exception().Message;
            // Membership can be queried after termination while the job handle
            // is retained. A surviving member cannot disassociate from its job.
            locker.in_job = job != null ? job.ContainsProcess(process) : false;
        }
        catch (Exception error) { locker.diagnostic_error = (locker.diagnostic_error == null ? "" : locker.diagnostic_error + "; ") + error.Message; }
        finally { CloseHandle(process); }
        return locker;
    }

    private static long FileTimeValue(FileTime time) { return ((long)time.High << 32) | time.Low; }

    private static void Check(uint error, string operation)
    {
        if (error != 0) throw new Win32Exception((int)error, operation + ": " + new Win32Exception((int)error).Message + " (" + error + ")");
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FileTime { public uint Low, High; }
    [StructLayout(LayoutKind.Sequential)]
    private struct RmUniqueProcess { public uint ProcessId; public FileTime StartTime; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct RmProcessInfo
    {
        public RmUniqueProcess Process;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string ApplicationName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string ServiceShortName;
        public uint ApplicationType, AppStatus, SessionId;
        [MarshalAs(UnmanagedType.Bool)] public bool Restartable;
    }

    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    private static extern uint RmStartSession(out uint session, uint flags, StringBuilder key);
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    private static extern uint RmRegisterResources(uint session, uint fileCount, string[] files,
        uint applicationCount, IntPtr applications, uint serviceCount, IntPtr services);
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    private static extern uint RmGetList(uint session, out uint needed, ref uint count,
        [In, Out] RmProcessInfo[] processes, out uint rebootReasons);
    [DllImport("rstrtmgr.dll")]
    private static extern uint RmEndSession(uint session);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint access, bool inherit, uint processId);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetProcessTimes(IntPtr process, out FileTime creation, out FileTime exit,
        out FileTime kernel, out FileTime user);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder image, ref uint length);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr process, uint milliseconds);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);
}
