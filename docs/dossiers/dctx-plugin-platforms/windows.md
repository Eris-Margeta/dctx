# Windows confinement profile

PL-R · 2026-10-16 · DX-PLUGIN §4.2 and PL-C S-ADMISSION/PP4.
API feasibility from existing knowledge; no Win32 source or native tests here.
PL-A/PL-REL consume this; x64 Windows native execution remains unsupported.

## AppContainer plus job, not job alone

A job object supplies process grouping, limits and termination. It does not
prevent a member from reading the user's files, opening a network connection,
reading credentials or invoking allowed services. A low-integrity token alone
also does not prevent reads. Neither qualifies third-party execution.

The candidate is an AppContainer security token with no network capabilities,
an exact filesystem/IPC ACL inventory and a nonbreakaway job. Consider LPAC
(less privileged AppContainer) to avoid resources broadly granted to
`ALL APPLICATION PACKAGES`; ordinary AppContainer access must be measured, not
assumed to be a private empty filesystem. Named pipes, COM/RPC and shared kernel
objects can confer authority separately from file ACLs.

Before the first plugin instruction:

1. Bind verified executable, DLLs and runtime resources in a namespace protected
   against writes/rename/delete, with read/execute only to the intended sandbox
   SID. No host home/project/cache ACLs, device paths, credentials or shared
   writable install root. Any container state directory is private and bounded.
2. Prepare `STARTUPINFOEX` security capabilities (AppContainer SID, empty
   capability list), explicit inherited pipe-handle list and child-process
   restriction policy. Inherit no process/token/job/directory/network handles.
3. Create with `EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED`, an explicit
   absolute application path and a freshly built environment block. Proper
   Windows argument serialization is required; never use `cmd.exe`, PowerShell,
   ShellExecute, file associations or PATH resolution for the worker.
4. Assign the suspended process to a job with kill-on-close, active-process
   limit 1 and no breakaway/silent-breakaway permission. Reject incompatible
   parent-job nesting or assignment failure, terminate the suspended process
   and keep execution unavailable. The kill handle must remain noninheritable.
5. Confirm security state, binding and audit intent, then resume the primary
   thread. Applying token/job restrictions after ordinary `CreateProcess`
   execution is already too late for PE TLS callbacks/DLL initialization.

Windows versions support differing creation attributes; the exact selected
SDK/build must implement the required combination. Only the named Windows 11
x64 fixture profile is proposed here; old token/job examples do not imply it.

## Network and descendants

Grant none of `internetClient`, `internetClientServer` or
`privateNetworkClientServer`. Check IPv4/IPv6, TCP/UDP and loopback behavior.
Loopback exemptions, pre-opened sockets and broker services can defeat a
simple capability story. Inspect/refuse conflicting exemptions using an
approved safe API or provide evidence of an independent denial. The installer
may have HTTPS access; its connections/credentials cannot cross into workers.

Combine child-process policy with the one-process nonbreakaway job. A job limit
alone is not a complete no-descendant API policy, and killing the parent PID
alone does not reap its tree. Deny remote process/thread injection and handle
duplication into the host through the token/ACL boundary. Test COM/launch
brokers as well as direct process creation. Job membership and kill-on-close
must survive cancellation and abnormal host death; inherited job handles must
not keep the group alive. Report `reap_pending` until actual exit is observed.

The same restrictions apply to a separately brokered Git invocation with its
own narrowly granted repository resources. Git for Windows has a larger
runtime/helper footprint than one executable; do not grant its whole install
tree execution rights or permit shells merely to make tests pass.

## Binding and unsafe boundary

Use safe handle APIs that inspect file ID, volume, reparse information and
link count without following hostile junctions. Hold deny-write/delete sharing
modes on executable/runtime files and protect all parent directory components
against replacement. Existing writable handles/mappings must be accounted for;
an identity comparison followed by an unlocked path launch is insufficient.
Reject reparse points, alternate data streams and ambiguous DOS/device names.
S-FILE's general compare-and-replace problem is detailed in
[bindings.md](bindings.md); `ReplaceFileW` is not a conditional compare-and-swap.

`windows = 0.62.2` and `windows-sys = 0.61.2` are exact lockfile candidates, not
safe high-level process sandboxes. The raw creation, token, ACL, attribute-list
and handle calls usually require unsafe calls and pointer lifetime discipline.
No approved safe wrapper for the required transaction is established here.
Adding a first-party unsafe adapter, copying bindings or adding a C helper
would violate the safety boundary. See [dependencies.md](dependencies.md).

The mechanisms can structurally fit one dctx `.exe` plus Windows system DLLs;
AppContainer does not inherently require shipping a second service executable.
This is feasibility, not an implementation claim. Actual distribution must
inventory the CRT and all DLL loading, sign the containing release, preserve
the early worker entry and pass per-build evidence. Current result remains
`UnsupportedPlatform` for third-party enablement.

Reference locators (not fetched): Microsoft documentation for
[AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation),
[job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[process attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
`CreateProcessW`, `CreateFileW`, `GetFileInformationByHandleEx` and `ReplaceFileW`.
