// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func isolateDiscovery(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	for _, key := range []string{"HOME", "USERPROFILE", "XDG_CACHE_HOME", "LOCALAPPDATA"} {
		t.Setenv(key, root)
	}
	t.Setenv("DCTX_BIN", "")
	if err := os.Unsetenv("DCTX_BIN"); err != nil {
		t.Fatal(err)
	}
	return root
}

func fakeExecutable(t *testing.T, path string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("not run by discovery tests\n"), 0o755); err != nil {
		t.Fatal(err)
	}
}

func TestDiscoveryOrderAndHardFailure(t *testing.T) {
	root := isolateDiscovery(t)
	cache, err := CacheBinary()
	if err != nil {
		t.Fatal(err)
	}
	wantSuffix := filepath.Join("dctx", "bin", Version, platformKey(), executableName())
	if !strings.HasSuffix(cache, wantSuffix) || !strings.HasPrefix(cache, root) {
		t.Fatal("incorrect cache layout:", cache)
	}
	// A PATH executable must never satisfy resolution or be downloaded implicitly.
	pathBinary := filepath.Join(root, "path", executableName())
	fakeExecutable(t, pathBinary)
	t.Setenv("PATH", filepath.Dir(pathBinary))
	_, err = ResolveBinary("")
	var missing *BinaryNotFoundError
	if !errors.As(err, &missing) {
		t.Fatalf("expected hard discovery error, got %v", err)
	}
	for _, text := range []string{cache, "https://github.com/Eris-Margeta/dctx/releases/tag/v" + Version, "SHA256SUMS", "DCTX_BIN", "cargo install --locked --git https://github.com/Eris-Margeta/dctx --tag v" + Version + " dctx-cli"} {
		if !strings.Contains(err.Error(), text) {
			t.Errorf("missing install instruction %q", text)
		}
	}
	if _, err := os.Stat(filepath.Dir(cache)); !os.IsNotExist(err) {
		t.Fatal("discovery created cache directory")
	}
	fakeExecutable(t, cache)
	if got, err := ResolveBinary(""); err != nil || got != cache {
		t.Fatalf("cache resolution: %q %v", got, err)
	}
	envBinary := filepath.Join(root, "env", executableName())
	fakeExecutable(t, envBinary)
	t.Setenv("DCTX_BIN", envBinary)
	if got, err := ResolveBinary(""); err != nil || got != envBinary {
		t.Fatalf("environment resolution: %q %v", got, err)
	}
	explicit := filepath.Join(root, "explicit", executableName())
	fakeExecutable(t, explicit)
	if got, err := ResolveBinary(explicit); err != nil || got != explicit {
		t.Fatalf("explicit resolution: %q %v", got, err)
	}
	t.Setenv("DCTX_BIN", filepath.Join(root, "absent"))
	if _, err := ResolveBinary(""); err == nil {
		t.Fatal("broken override fell through to cache")
	}
	if _, err := ResolveBinary(filepath.Join(root, "absent")); err == nil {
		t.Fatal("broken explicit path fell through")
	}
	t.Setenv("DCTX_BIN", "")
	if _, err := ResolveBinary(""); err == nil {
		t.Fatal("empty environment override fell through")
	}
	if _, err := ResolveBinary(root); err == nil {
		t.Fatal("directory accepted as executable")
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(explicit, 0o644); err != nil {
			t.Fatal(err)
		}
		if _, err := ResolveBinary(explicit); err == nil {
			t.Fatal("non-executable accepted")
		}
	}
}

func TestRelativeOverrideUsesCallerDirectory(t *testing.T) {
	root := isolateDiscovery(t)
	binary := filepath.Join(root, executableName())
	fakeExecutable(t, binary)
	cwd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	relative, err := filepath.Rel(cwd, binary)
	if err != nil {
		t.Fatal(err)
	}
	if got, err := ResolveBinary(relative); err != nil || got != binary {
		t.Fatalf("relative resolution: %q %v", got, err)
	}
}
