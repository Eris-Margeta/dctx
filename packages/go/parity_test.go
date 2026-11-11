// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

func parityBinary(t *testing.T) string {
	t.Helper()
	binary := os.Getenv("DCTX_TEST_BIN")
	if binary == "" {
		binary = filepath.Join("..", "..", "target", "debug", "dctx-cli")
		if runtime.GOOS == "windows" {
			binary += ".exe"
		}
	}
	abs, err := filepath.Abs(binary)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(abs); err != nil {
		message := "real-engine parity requires cargo build --locked -p dctx-cli and DCTX_TEST_BIN pointing to that executable"
		if os.IsNotExist(err) && os.Getenv("CI") != "" && os.Getenv("DCTX_REQUIRE_ENGINE_TESTS") != "1" && os.Getenv("DCTX_TEST_BIN") == "" {
			t.Skip("CI job has no engine binary: " + message)
		}
		t.Fatal(message, ":", err)
	}
	return abs
}

func parityEnvironment(t *testing.T) []string {
	t.Helper()
	root := t.TempDir()
	// Explicit isolation prevents installed configuration and caches from entering parity.
	env := []string{"DCTX_AUTO_CLEAN=0", "NO_COLOR=1"}
	for _, key := range []string{"PATH", "SystemRoot", "SYSTEMROOT", "WINDIR"} {
		if value, ok := os.LookupEnv(key); ok {
			env = append(env, key+"="+value)
		}
	}
	for key, dir := range map[string]string{
		"HOME": "home", "USERPROFILE": "home", "XDG_CONFIG_HOME": "config", "APPDATA": "config",
		"XDG_CACHE_HOME": "cache", "XDG_DATA_HOME": "data", "LOCALAPPDATA": "local",
		"TMPDIR": "tmp", "TMP": "tmp", "TEMP": "tmp", "DCTX_CACHE_DIR": "engine-cache", "DCTX_MAP_CACHE": "engine-map-cache",
	} {
		path := filepath.Join(root, dir)
		if err := os.MkdirAll(path, 0o755); err != nil {
			t.Fatal(err)
		}
		env = append(env, key+"="+path)
	}
	return env
}

func cliWords(verb Verb) []string {
	switch verb {
	case VerbPack, VerbContext:
		return []string{"pack"}
	case VerbLanguages:
		return []string{"languages"}
	case VerbCacheStatus:
		return []string{"cache", "status"}
	default:
		return []string{"map", string(verb)}
	}
}

func runCLI(t *testing.T, binary, repo string, env []string, verb Verb, args []string) ([]byte, []byte, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	words := append(cliWords(verb), args...)
	command := exec.CommandContext(ctx, binary, append(words, "--json")...)
	command.Dir, command.Env = repo, env
	var stderr bytes.Buffer
	command.Stderr = &stderr
	stdout, err := command.Output()
	if err == nil {
		return stdout, stderr.Bytes(), 0
	}
	var exit *exec.ExitError
	if !errors.As(err, &exit) || ctx.Err() != nil {
		t.Fatal("CLI did not complete:", err, ctx.Err())
	}
	return stdout, stderr.Bytes(), exit.ExitCode()
}

func realClient(t *testing.T) (*Client, string, string, []string) {
	t.Helper()
	binary := parityBinary(t)
	repo, err := filepath.Abs(filepath.Join("..", "..", "crates", "dctx", "tests", "fixtures", "repository"))
	if err != nil {
		t.Fatal(err)
	}
	env := parityEnvironment(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	client, err := New(ctx, repo, Options{Binary: binary, Env: env})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := client.Close(); err != nil {
			t.Error(err)
		}
	})
	return client, binary, repo, env
}

func TestEveryVerbMatchesCLIBytes(t *testing.T) {
	client, binary, repo, env := realClient(t)
	// Rust RPC parity corpus, extended to cover every advertised verb.
	cases := []struct {
		verb Verb
		args []string
	}{
		{VerbFind, []string{"ping"}}, {VerbFind, []string{"not_present_zz"}}, {VerbFind, []string{"ping", "--limit", "1"}},
		{VerbMap, []string{"m:src/lib.rs"}}, {VerbRead, []string{"s:src/lib.rs#ping"}}, {VerbRead, []string{"m:src/lib.rs", "--lines", "1-2"}},
		{VerbSkeleton, []string{"m:src/lib.rs"}}, {VerbExplain, []string{"s:src/lib.rs#ping"}}, {VerbNeighbors, []string{"s:src/lib.rs#ping"}},
		{VerbGrep, []string{"pub"}}, {VerbHubs, nil}, {VerbPath, []string{"m:src/lib.rs", "s:src/lib.rs#ping"}},
		{VerbTrace, []string{"s:src/lib.rs#ping", "s:src/lib.rs#ping"}},
		// The isolated ping has no directed path to its module or incoming callers.
		{VerbTrace, []string{"s:src/lib.rs#ping", "m:src/lib.rs", "--max-depth", "1", "--max-paths", "1", "--max-bytes", "256"}},
		{VerbImpact, []string{"s:src/lib.rs#ping"}},
		{VerbImpact, []string{"s:src/lib.rs#ping", "--max-depth", "1", "--max-sites", "1", "--max-bytes", "256"}},
		{VerbPack, nil}, {VerbContext, []string{"--for", "`ping`"}}, {VerbLanguages, nil}, {VerbCacheStatus, nil},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	hello, err := client.Hello(ctx)
	if err != nil {
		t.Fatal(err)
	}
	seen := map[Verb]bool{VerbHello: true}
	for i, test := range cases {
		t.Run(fmt.Sprintf("%02d_%s", i, test.verb), func(t *testing.T) {
			stdout, stderr, status := runCLI(t, binary, repo, env, test.verb, test.args)
			if status != 0 {
				t.Fatalf("CLI exit %d: %s", status, stderr)
			}
			output, err := client.Call(ctx, test.verb, test.args...)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(output, stdout) {
				t.Fatalf("CLI/Go bytes differ for %s", test.verb)
			}
			seen[test.verb] = true
		})
	}
	if len(seen) != len(hello.Verbs) {
		t.Fatalf("parity cases do not cover advertised verbs: %v vs %v", seen, hello.Verbs)
	}
	for _, verb := range hello.Verbs {
		if !seen[verb] {
			t.Errorf("no parity case for advertised verb %s", verb)
		}
	}
}

func TestEngineErrorsMatchCLI(t *testing.T) {
	client, binary, repo, env := realClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	codes := map[int]bool{}
	for _, test := range []struct {
		verb Verb
		args []string
	}{
		{VerbRead, []string{"m:absent.rs"}}, {VerbFind, []string{""}}, {VerbFind, []string{"ping", "--place", "missing-directory"}},
		{VerbLanguages, []string{"--bad"}}, {VerbCacheStatus, []string{"--bad"}}, {VerbPack, []string{"--ext", "rs"}},
		{VerbContext, []string{"--for", "`absent`"}}, {VerbPack, []string{"--for", "--json"}},
	} {
		t.Run(string(test.verb)+strings.Join(test.args, "_"), func(t *testing.T) {
			stdout, stderr, status := runCLI(t, binary, repo, env, test.verb, test.args)
			_, err := client.Call(ctx, test.verb, test.args...)
			var failure *EngineError
			if !errors.As(err, &failure) {
				t.Fatalf("expected EngineError: %v", err)
			}
			if failure.Code != status || !bytes.Equal([]byte(failure.Output), stdout) {
				t.Fatalf("lost CLI status/output: %+v, status %d", failure, status)
			}
			diagnostic := "❌ Error: " + failure.Message + "\n"
			if failure.Hint != "" {
				diagnostic += "hint: " + failure.Hint + "\n"
			}
			if !bytes.Equal([]byte(diagnostic), stderr) {
				t.Fatalf("diagnostic differs: %q vs %q", diagnostic, stderr)
			}
			codes[failure.Code] = true
		})
	}
	for _, code := range []int{1, 2, 3} {
		if !codes[code] {
			t.Errorf("CLI error code %d not covered", code)
		}
	}
	output, err := client.Languages(ctx, "--json")
	if err != nil {
		t.Fatal("error poisoned session:", err)
	}
	stdout, _, _ := runCLI(t, binary, repo, env, VerbLanguages, nil)
	if !bytes.Equal(output, stdout) {
		t.Fatal("explicit --json parity failed")
	}
}
