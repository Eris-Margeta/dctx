#!/usr/bin/env python3
"""Bounded redacted tree/history scanner. No values or raw diffs are emitted.
Exit 0: no plausible candidates; 1: review needed; 2: incomplete/error.
Protected filenames are never read. Only this trusted scanner executes.
"""
import collections
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time

MAX_COMMITS = 10000
MAX_ENTRIES = 5000000
MAX_BLOB = 8 * 1024 * 1024
MAX_BYTES = 512 * 1024 * 1024
MAX_SECONDS = 300
MAX_REPORTS = 250
START = time.monotonic()
COUNTS = collections.Counter()
REPORTS = set()
EMITTED = collections.Counter()
CACHE = {}

TOKEN = re.compile(
    r'\b(?:AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{20,255}'
    r'|github_pat_[A-Za-z0-9_]{22,255}|glpat-[A-Za-z0-9_-]{20,255}'
    r'|xox[baprs]-[A-Za-z0-9-]{10,255}|sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,255}'
    r'|[rs]k_live_[A-Za-z0-9]{16,255})\b'
)
ASSIGN = re.compile(
    r'''(?ix)\b(?:api[_-]?key|access[_-]?token|auth[_-]?token|secret[_-]?key|
    client[_-]?secret|password|passwd|private[_-]?key)\b["']?\s*[:=]\s*
    (?:["']([^"'\r\n]{8,256})["']|([A-Za-z0-9_+./=-]{16,256}))'''
)
PEM_PRIVATE = re.compile(r'-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----')
URL = re.compile(r'\b[a-z][a-z0-9+.-]*://[^\s/:@]+:([^\s/@]{4,256})@', re.I)
PLACEHOLDER = re.compile(
    r'(?i)^(?:test[-_]?|dummy[-_]?|fake[-_]?|example[-_]?|sample[-_]?|mock[-_]?|'
    r'your[-_]|replace[-_]|changeme|placeholder|redacted|<|\$\{|\{\{)|'
    r'(?i:EXAMPLE)$'
)


def budget():
    if time.monotonic() - START > MAX_SECONDS:
        raise RuntimeError('time_limit')


def git(*args):
    """Git metadata only, with output cap and no raw stderr propagation."""
    budget()
    proc = subprocess.Popen(['git', '--no-replace-objects', *args], stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL)
    try:
        data = bytearray()
        while True:
            budget()
            block = proc.stdout.read(65536)
            if not block:
                break
            data.extend(block)
            if len(data) > 64 * 1024 * 1024:
                raise RuntimeError('metadata_limit')
        if proc.wait(timeout=10):
            raise RuntimeError('git_failed')
        return bytes(data)
    finally:
        if proc.poll() is None:
            proc.kill()
        proc.wait()
        proc.stdout.close()


def protected(path):
    parts = Path(path.lower()).parts
    name = parts[-1]
    return ('auth.json' in name or 'keychain' in name
            or ('private' in name and 'key' in name)
            or name in ('auth.json', 'credentials.json', 'credentials', '.netrc', '.npmrc',
                     '.pypirc', 'config.json') and (name != 'config.json' or '.docker' in parts)
            or any(x in parts for x in ('.ssh', '.gnupg', 'keychains'))
            or (not name.endswith('.pub') and name.startswith(('id_rsa', 'id_dsa', 'id_ecdsa', 'id_ed25519')))
            or name.endswith(('.pem', '.key', '.p12', '.pfx', '.keystore', '.keychain',
                              '.keychain-db', '.ppk', '.jks', '.priv')))


def filename_rules(path):
    name = Path(path.lower()).name
    if protected(path):
        return [('protected-filename:needs-review', 0)]
    if name.startswith('.env') and name != '.env.example':
        return [('environment-filename:needs-review', 0)]
    return []


def fixture(value):
    return (bool(PLACEHOLDER.search(value)) or (value.lower().startswith('sk-test-')
                and 'dctx' in value.lower() and 'secret' in value.lower())
            or len(set(value)) <= 3
            or value.lower() in ('password', 'secret', 'supersecret', 'secret123',
                                'verysecret', 'mysecret', '0123456789abcdef')
            or any(c in value for c in ('${', '{{', '<', '>')))


def analyze(data):
    if b'\0' in data:
        COUNTS['binary_blobs'] += 1
        return []
    COUNTS['text_blobs'] += 1
    results = set()
    for number, line in enumerate(data.decode('utf-8', 'replace').splitlines(), 1):
        if PEM_PRIVATE.search(line):
            results.add(('pem-private-header:plausible', number))
        if '-----BEGIN PUBLIC KEY-----' in line or re.search(r'\bssh-(?:rsa|ed25519)\s+[A-Za-z0-9+/]{20,}', line):
            results.add(('public-key:non-secret', number))
        for match in TOKEN.finditer(line):
            results.add(('provider-token:' + ('fixture' if fixture(match[0]) else 'plausible'), number))
        for match in ASSIGN.finditer(line):
            value = match[1] or match[2]
            # Expressions, declarations and natural language are not literal credentials.
            if not re.fullmatch(r'[A-Za-z0-9_+./=:@!$%&*?-]{8,256}', value):
                continue
            entropy = -sum((n / len(value)) * math.log2(n / len(value))
                           for n in collections.Counter(value).values())
            if fixture(value):
                results.add(('credential-assignment:fixture', number))
            elif entropy >= 3.4 and len(value) >= 16:
                results.add(('credential-assignment:plausible', number))
        for match in URL.finditer(line):
            results.add(('url-credential:' + ('fixture' if fixture(match[1]) else 'plausible'), number))
        if re.search(r'/(?:Users|home)/[A-Za-z0-9_.-]+/', line):
            results.add(('local-user-path:privacy-review', number))
    return sorted(results)


def report(path, revision, findings):
    for rule, line in findings:
        COUNTS[rule] += 1
        # Deduplicate repeat snapshots, retaining a representative containing revision.
        key = (rule, path, line)
        if key not in REPORTS:
            REPORTS.add(key)
            priority = 'candidate' if rule.endswith((':plausible', ':needs-review')) else 'context'
            if EMITTED[priority] < MAX_REPORTS:
                EMITTED[priority] += 1
                safe_path = URL.sub('[REDACTED]', ASSIGN.sub('[REDACTED]',
                                    TOKEN.sub('[REDACTED]', path)))
                print(json.dumps(dict(rule=rule, path=safe_path, line=line, revision=revision)), flush=True)


def scan_blob(path, oid, size, revision, reader):
    findings = filename_rules(path)
    if protected(path):
        COUNTS['protected_entries_not_read'] += 1
    elif size > MAX_BLOB:
        COUNTS['oversize_entries_not_read'] += 1
    else:
        if oid not in CACHE:
            if COUNTS['bytes_read'] + size > MAX_BYTES:
                raise RuntimeError('byte_limit')
            data = reader()
            COUNTS['bytes_read'] += len(data)
            COUNTS['unique_contents_scanned'] += 1
            CACHE[oid] = analyze(data)
        findings += CACHE[oid]
    report(path, revision, findings)


def main():
    def timeout(signum, frame):
        raise RuntimeError('time_limit')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(MAX_SECONDS)
    root = Path(git('rev-parse', '--show-toplevel').decode().strip())
    os.chdir(root)
    head = git('rev-parse', 'HEAD').decode().strip()
    refs_before = git('for-each-ref', '--format=%(objectname) %(refname)')
    shallow = git('rev-parse', '--is-shallow-repository').strip() == b'true'
    commits = git('rev-list', '--all', 'HEAD').decode().splitlines()
    if len(commits) > MAX_COMMITS:
        raise RuntimeError('commit_limit')
    # cat-file is asked only for explicitly allowed, size-checked regular blobs.
    batch = subprocess.Popen(['git', '--no-replace-objects', 'cat-file', '--batch'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    def blob(oid, size):
        batch.stdin.write(oid.encode() + b'\n')
        batch.stdin.flush()
        header = batch.stdout.readline(256).split()
        if len(header) != 3 or header[1] != b'blob' or int(header[2]) != size:
            raise RuntimeError('blob_metadata')
        data = batch.stdout.read(size)
        if len(data) != size or batch.stdout.read(1) != b'\n':
            raise RuntimeError('blob_truncated')
        return data
    try:
        for raw in git('ls-files', '-z').split(b'\0'):
            if not raw:
                continue
            path = os.fsdecode(raw)
            COUNTS['tree_paths'] += 1
            if protected(path):
                scan_blob(path, '', 0, 'WORKTREE', lambda: b'')
                continue
            # O_NOFOLLOW also protects against a symlink substituted after lstat.
            try:
                info = os.lstat(path)
                if not stat.S_ISREG(info.st_mode):
                    COUNTS['tree_nonregular_not_read'] += 1
                    continue
                if info.st_size > MAX_BLOB:
                    scan_blob(path, '', info.st_size, 'WORKTREE', lambda: b'')
                    continue
                fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
                with os.fdopen(fd, 'rb') as source:
                    data = source.read(MAX_BLOB + 1)
                if len(data) > MAX_BLOB:
                    raise RuntimeError('changing_file_size')
                scan_blob(path, hashlib.sha256(data).hexdigest(), len(data), 'WORKTREE', lambda: data)
            except FileNotFoundError:
                COUNTS['tree_missing_not_read'] += 1
        for revision in commits:
            budget()
            COUNTS['commits_scanned'] += 1
            for entry in git('ls-tree', '-rz', '-l', revision).split(b'\0'):
                if not entry:
                    continue
                meta, raw_path = entry.split(b'\t', 1)
                mode, kind, oid, size = meta.split()
                COUNTS['history_entries'] += 1
                if COUNTS['history_entries'] > MAX_ENTRIES:
                    raise RuntimeError('entry_limit')
                if kind != b'blob' or mode == b'120000':
                    COUNTS['history_nonregular_not_read'] += 1
                    continue
                path, oid, size = os.fsdecode(raw_path), oid.decode(), int(size)
                scan_blob(path, oid, size, revision, lambda: blob(oid, size))
    finally:
        batch.stdin.close()
        batch.stdout.close()
        if batch.poll() is None:
            batch.terminate()
        batch.wait(timeout=10)
    changed = (head != git('rev-parse', 'HEAD').decode().strip()
               or refs_before != git('for-each-ref', '--format=%(objectname) %(refname)'))
    incomplete = shallow or changed or any(COUNTS[k] for k in (
        'oversize_entries_not_read', 'tree_missing_not_read'))
    candidates = sum(v for k, v in COUNTS.items() if k.endswith((':plausible', ':needs-review')))
    print(json.dumps(dict(summary=dict(sorted(COUNTS.items())), head=head,
                         shallow=shallow, refs_changed=changed, incomplete=incomplete,
                         unique_reports=len(REPORTS), reports_truncated=len(REPORTS) > sum(EMITTED.values()),
                         history='all local refs plus HEAD; no reflogs, unreachable objects, LFS payloads or remote-only refs')))
    return 2 if incomplete else int(bool(candidates))


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception:
        # Never echo exception strings: they may include source content or values.
        print(json.dumps({'summary': dict(sorted(COUNTS.items())), 'incomplete': True}))
        print(json.dumps({'rule': 'scan-incomplete', 'path': '.', 'line': 0, 'revision': 'SCAN'}))
        sys.exit(2)
