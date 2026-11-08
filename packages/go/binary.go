// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
)

// BinaryNotFoundError means the authoritative engine location is unusable.
type BinaryNotFoundError struct {
	Path  string
	Cause error
}

func (e *BinaryNotFoundError) Error() string {
	return fmt.Sprintf("dctx engine unavailable at %q: %v. %s", e.Path, e.Cause, InstallInstructions())
}

func (e *BinaryNotFoundError) Unwrap() error { return e.Cause }

func platformKey() string {
	system := runtime.GOOS
	arch := runtime.GOARCH
	switch arch {
	case "amd64":
		arch = "x86_64"
	case "arm64":
		arch = "aarch64"
	}
	return system + "_" + arch
}

func executableName() string {
	if runtime.GOOS == "windows" {
		return "dctx.exe"
	}
	return "dctx"
}

// CacheBinary returns the versioned per-user engine location. It creates nothing.
func CacheBinary() (string, error) {
	root, err := os.UserCacheDir()
	if err != nil && runtime.GOOS == "windows" {
		if home, homeErr := os.UserHomeDir(); homeErr == nil {
			root, err = filepath.Join(home, "AppData", "Local"), nil
		}
	}
	if err != nil {
		return "", err
	}
	if !filepath.IsAbs(root) {
		return "", fmt.Errorf("user cache directory must be absolute")
	}
	return filepath.Join(root, "dctx", "bin", Version, platformKey(), executableName()), nil
}

// InstallInstructions describes explicit installation; this package never downloads.
func InstallInstructions() string {
	cache, err := CacheBinary()
	if err != nil {
		cache = "<user-cache>/dctx/bin/" + Version + "/" + platformKey() + "/" + executableName()
	}
	return "Download engine " + Version + " from https://github.com/Eris-Margeta/dctx/releases/tag/v" + Version +
		", verify the archive against SHA256SUMS, extract it, then set DCTX_BIN to the absolute path of " + executableName() +
		" or place that executable at " + cache + ". For a tagged source install, run: cargo install --locked --git https://github.com/Eris-Margeta/dctx --tag v" + Version +
		" dctx-cli; then set DCTX_BIN to the absolute path of the installed dctx-cli executable. No automatic download is performed."
}

// ResolveBinary selects an explicit path, otherwise DCTX_BIN, otherwise CacheBinary.
// Overrides are authoritative. Relative overrides resolve in the caller's current
// directory. PATH is never searched and discovery never installs an engine.
func ResolveBinary(explicit string) (string, error) {
	candidate := explicit
	if candidate == "" {
		var configured bool
		candidate, configured = os.LookupEnv("DCTX_BIN")
		if !configured {
			var err error
			candidate, err = CacheBinary()
			if err != nil {
				return "", &BinaryNotFoundError{Cause: err}
			}
		}
	}
	if candidate == "" {
		return "", &BinaryNotFoundError{Cause: fmt.Errorf("empty executable path")}
	}
	abs, err := filepath.Abs(candidate)
	if err != nil {
		return "", &BinaryNotFoundError{Path: candidate, Cause: err}
	}
	info, err := os.Stat(abs)
	if err == nil && (!info.Mode().IsRegular() || (runtime.GOOS != "windows" && info.Mode().Perm()&0o111 == 0)) {
		err = fmt.Errorf("not a regular executable file")
	}
	if err != nil {
		return "", &BinaryNotFoundError{Path: abs, Cause: err}
	}
	return abs, nil
}
