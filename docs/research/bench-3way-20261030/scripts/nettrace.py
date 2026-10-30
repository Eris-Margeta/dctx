#!/usr/bin/env python3
"""Minimal strace -f replacement (x86_64, ptrace via ctypes, stdlib only): records socket/connect/sendto/sendmsg/bind
attempts of a command and all its descendants. strace is not installed on the bench host and we may not install it.

usage: nettrace.py OUT.json -- CMD [ARGS...]
Writes OUT.json = {rc, events:[{pid,sys,family,addr,port,ret}], summary:{...}}. The traced command's stdout/stderr go to /dev/null.
"""
import ctypes, json, os, signal, struct, sys

libc = ctypes.CDLL(None, use_errno=True)
libc.ptrace.restype = ctypes.c_long
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
PT_TRACEME, PT_CONT, PT_GETREGS, PT_SETOPTIONS, PT_SYSCALL = 0, 7, 12, 0x4200, 24
OPTS = 1 | 2 | 4 | 8 | 0x10 | 0x100000  # SYSGOOD FORK VFORK CLONE EXEC EXITKILL
__WALL = 0x40000000
SYS = {41: "socket", 42: "connect", 44: "sendto", 46: "sendmsg", 49: "bind"}
FAM = {1: "AF_UNIX", 2: "AF_INET", 10: "AF_INET6", 16: "AF_NETLINK"}


class Regs(ctypes.Structure):
    _fields_ = [(n, ctypes.c_ulonglong) for n in
                "r15 r14 r13 r12 rbp rbx r11 r10 r9 r8 rax rcx rdx rsi rdi orig_rax rip cs eflags rsp ss fs_base gs_base ds es fs gs".split()]


def readmem(pid, addr, n):
    try:
        with open(f"/proc/{pid}/mem", "rb", buffering=0) as f:
            f.seek(addr)
            return f.read(n)
    except OSError:
        return b""


def sockaddr(pid, addr, ln):
    if not addr or ln < 2:
        return None
    b = readmem(pid, addr, min(max(ln, 2), 128))
    if len(b) < 2:
        return None
    fam = struct.unpack("H", b[:2])[0]
    d = {"family": FAM.get(fam, str(fam))}
    if fam == 2 and len(b) >= 8:
        d["port"] = struct.unpack(">H", b[2:4])[0]
        d["addr"] = ".".join(str(x) for x in b[4:8])
    elif fam == 10 and len(b) >= 24:
        d["port"] = struct.unpack(">H", b[2:4])[0]
        d["addr"] = b[8:24].hex()
    elif fam == 1:
        d["addr"] = b[2:].split(b"\0")[0].decode("utf-8", "replace") or "(abstract)"
    return d


def main():
    out = sys.argv[1]
    cmd = sys.argv[sys.argv.index("--") + 1:]
    pid = os.fork()
    if pid == 0:
        dn = os.open("/dev/null", os.O_RDWR)
        os.dup2(dn, 0); os.dup2(dn, 1); os.dup2(dn, 2)
        libc.ptrace(PT_TRACEME, 0, None, None)
        os.kill(os.getpid(), signal.SIGSTOP)
        os.execvp(cmd[0], cmd)
    events, insys, pending = [], {}, {}
    rc, root = None, pid
    _, st = os.waitpid(pid, __WALL)
    libc.ptrace(PT_SETOPTIONS, pid, None, ctypes.c_void_p(OPTS))
    libc.ptrace(PT_SYSCALL, pid, None, None)
    alive = {pid}
    while alive:
        try:
            p, st = os.waitpid(-1, __WALL)
        except ChildProcessError:
            break
        if os.WIFEXITED(st) or os.WIFSIGNALED(st):
            alive.discard(p)
            insys.pop(p, None)
            if p == root:
                rc = os.WEXITSTATUS(st) if os.WIFEXITED(st) else 128 + os.WTERMSIG(st)
            continue
        if not os.WIFSTOPPED(st):
            continue
        alive.add(p)
        sig = os.WSTOPSIG(st)
        inject = 0
        if sig == (signal.SIGTRAP | 0x80):
            r = Regs()
            libc.ptrace(PT_GETREGS, p, None, ctypes.byref(r))
            if not insys.get(p):
                insys[p] = True
                nm = SYS.get(r.orig_rax)
                if nm:
                    ev = {"pid": p, "sys": nm}
                    if nm == "socket":
                        ev["family"] = FAM.get(r.rdi, str(r.rdi))
                    elif nm in ("connect", "bind"):
                        ev.update(sockaddr(p, r.rsi, r.rdx) or {})
                    elif nm == "sendto" and r.r8:
                        ev.update(sockaddr(p, r.r8, r.r9) or {})
                    elif nm == "sendmsg":
                        mh = readmem(p, r.rsi, 8)
                        if len(mh) == 8:
                            ptr = struct.unpack("Q", mh)[0]
                            ln = struct.unpack("I", readmem(p, r.rsi + 8, 4) or b"\0\0\0\0")[0]
                            ev.update(sockaddr(p, ptr, ln) or {})
                    pending[p] = ev
            else:
                insys[p] = False
                ev = pending.pop(p, None)
                if ev is not None:
                    rv = ctypes.c_longlong(r.rax).value
                    ev["ret"] = rv if rv >= 0 else -rv and "-" + (os.strerror(-rv) if -rv < 200 else str(rv))
                    events.append(ev)
        elif sig == signal.SIGTRAP or sig == signal.SIGSTOP:
            inject = 0
        else:
            inject = sig
        libc.ptrace(PT_SYSCALL, p, None, ctypes.c_void_p(inject))
    net = [e for e in events if e.get("family") in ("AF_INET", "AF_INET6")]
    summ = {"socket_calls": sum(1 for e in events if e["sys"] == "socket"),
            "families": sorted({e.get("family") for e in events if e["sys"] == "socket"}),
            "inet_connect_attempts": sorted({f'{e["sys"]} {e.get("addr")}:{e.get("port")} -> {e.get("ret")}'
                                             for e in net if e["sys"] in ("connect", "sendto", "sendmsg")}),
            "inet_attempt_count": sum(1 for e in net if e["sys"] in ("connect", "sendto", "sendmsg")),
            "unix_connects": sorted({e.get("addr") for e in events if e["sys"] == "connect" and e.get("family") == "AF_UNIX"})}
    json.dump({"rc": rc, "events": events[:400], "summary": summ}, open(out, "w"), indent=1)
    sys.exit(0)


if __name__ == "__main__":
    main()
