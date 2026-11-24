// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"
)

func parityBinary(t *testing.T) string {
	t.Helper()
	binary := os.Getenv("DCTX_TEST_BIN")
	if binary == "" && os.Getenv("DCTX_REQUIRE_ENGINE_TESTS") == "1" {
		t.Fatal("DCTX_REQUIRE_ENGINE_TESTS=1 requires DCTX_TEST_BIN")
	}
	if binary == "" {
		binary = filepath.Join("..", "..", "target", "debug", "dctx-cli")
		if runtime.GOOS == "windows" {
			binary += ".exe"
		}
	}
	abs, err := filepath.Abs(binary)
	if err != nil {
		t.Fatalf("resolve engine path: %q", err)
	}
	info, err := os.Stat(abs)
	if err != nil {
		message := "real-engine parity requires cargo build --locked -p dctx-cli and DCTX_TEST_BIN pointing to that executable"
		if os.IsNotExist(err) && os.Getenv("CI") != "" && os.Getenv("DCTX_REQUIRE_ENGINE_TESTS") != "1" && os.Getenv("DCTX_TEST_BIN") == "" {
			t.Skip("CI job has no engine binary: " + message)
		}
		t.Fatalf("%s: %q", message, err)
	}
	if !info.Mode().IsRegular() || (runtime.GOOS != "windows" && info.Mode()&0o111 == 0) {
		t.Fatalf("DCTX_TEST_BIN is not executable: %q", abs)
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
			t.Fatalf("create isolated directory: %q", err)
		}
		env = append(env, key+"="+path)
	}
	return env
}

func cliWords(verb Verb, args []string) []string {
	switch verb {
	case VerbContext:
		if slices.Contains(args, "--for") {
			return []string{"pack"}
		}
		return []string{"context"}
	case VerbPack:
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
	words := append(cliWords(verb, args), args...)
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
		t.Fatalf("CLI did not complete: %q", err)
	}
	return stdout, stderr.Bytes(), exit.ExitCode()
}

func realClient(t *testing.T) (*Client, string, string, []string) {
	t.Helper()
	binary := parityBinary(t)
	repo, err := filepath.Abs(filepath.Join("..", "..", "crates", "dctx", "tests", "fixtures", "repository"))
	if err != nil {
		t.Fatalf("resolve fixture path: %q", err)
	}
	env := parityEnvironment(t)
	return connectParity(t, binary, repo, env), binary, repo, env
}

func connectParity(t *testing.T, binary, repo string, env []string) *Client {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	client, err := New(ctx, repo, Options{Binary: binary, Env: env})
	if err != nil {
		t.Fatalf("connect to engine: %q", err)
	}
	t.Cleanup(func() {
		if err := client.Close(); err != nil {
			t.Errorf("close engine: %q", err)
		}
	})
	return client
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
		t.Fatalf("receive engine hello: %q", err)
	}
	seen := map[Verb]bool{VerbHello: true}
	for i, test := range cases {
		t.Run(fmt.Sprintf("%02d_%s", i, test.verb), func(t *testing.T) {
			stdout, stderr, status := runCLI(t, binary, repo, env, test.verb, test.args)
			if status != 0 {
				t.Fatalf("CLI exit %d: %q", status, stderr)
			}
			output, err := client.Call(ctx, test.verb, test.args...)
			if err != nil {
				t.Fatalf("engine call failed: %q", err)
			}
			if !bytes.Equal(output, stdout) {
				t.Fatalf("CLI/Go bytes differ for %s", test.verb)
			}
			seen[test.verb] = true
		})
	}
	if len(seen) != len(hello.Verbs) {
		t.Fatalf("parity cases do not cover advertised verbs: %q vs %q", fmt.Sprint(seen), hello.Verbs)
	}
	for _, verb := range hello.Verbs {
		if !seen[verb] {
			t.Errorf("no parity case for advertised verb %q", verb)
		}
	}
}

func TestEngineErrorsMatchCLI(t *testing.T) {
	client, binary, repo, env := realClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	codes := map[int]bool{}
	hostile := loadWrapperCases(t).HostileFlag
	for i, test := range []struct {
		verb Verb
		args []string
	}{
		{VerbRead, []string{"m:absent.rs"}}, {VerbFind, []string{""}}, {VerbFind, []string{"ping", "--place", "missing-directory"}},
		{VerbLanguages, []string{"--bad"}}, {VerbCacheStatus, []string{"--bad"}}, {VerbPack, []string{"--ext", "rs"}},
		{VerbContext, []string{"--for", "`absent`"}}, {VerbPack, []string{"--for", "--json"}},
		{VerbLanguages, []string{hostile}},
	} {
		t.Run(fmt.Sprintf("%02d_%s", i, test.verb), func(t *testing.T) {
			stdout, stderr, status := runCLI(t, binary, repo, env, test.verb, test.args)
			output, err := client.Call(ctx, test.verb, test.args...)
			failure := checkFailure(t, output, err, stdout, stderr, status)
			codes[failure.Code] = true
			if slices.Equal(test.args, []string{hostile}) &&
				(failure.Code != 2 || !strings.Contains(failure.Message, hostile) || failure.Hint == "") {
				t.Fatal("hostile argument did not produce a usage diagnostic with a hint")
			}
		})
	}
	for _, code := range []int{1, 2, 3} {
		if !codes[code] {
			t.Errorf("CLI error code %d not covered", code)
		}
	}
	output, err := client.Languages(ctx, "--json")
	if err != nil {
		t.Fatalf("error poisoned session: %q", err)
	}
	stdout, _, _ := runCLI(t, binary, repo, env, VerbLanguages, nil)
	if !bytes.Equal(output, stdout) {
		t.Fatal("explicit --json parity failed")
	}
}

func checkFailure(t *testing.T, output []byte, err error, stdout, stderr []byte, status int) *EngineError {
	t.Helper()
	var failure *EngineError
	if !errors.As(err, &failure) || output != nil {
		t.Fatalf("expected EngineError and nil result, got error %q and result %q", fmt.Sprint(err), output)
	}
	if failure.Code != status {
		t.Fatalf("engine failure code %d, CLI exit %d", failure.Code, status)
	}
	checkBytes(t, "failure stdout", []byte(failure.Output), stdout)
	diagnostic := "❌ Error: " + failure.Message + "\n"
	if failure.Hint != "" {
		diagnostic += "hint: " + failure.Hint + "\n"
	}
	checkBytes(t, "CLI diagnostic", []byte(diagnostic), stderr)
	return failure
}

func checkBytes(t *testing.T, description string, actual, expected []byte) {
	t.Helper()
	if !bytes.Equal(actual, expected) {
		t.Fatalf("%s: got %q, want %q", description, actual, expected)
	}
}

type pageCase struct {
	Verb     Verb
	Args     []string
	Expected map[string]any
}

type wrapperCases struct {
	Files   map[string]string
	Context struct {
		Good, Gone, Session, Target, Content string
	}
	Reads []struct {
		Target, File string
		Lines        [2]int
	}
	Outline struct {
		Target string
		IDs    [2]string
	}
	Graph      []pageCase
	Extraction struct {
		OversizedFile string `json:"oversized_file"`
		Size          int64
		Partial       pageCase
		Opaque        pageCase
		RefusedRead   pageCase `json:"refused_read"`
	}
	HostileFlag string `json:"hostile_flag"`
}

func loadWrapperCases(t *testing.T) wrapperCases {
	t.Helper()
	path := filepath.Join("..", "..", "tools", "engine-contract", "wrapper-cases", "pages.json")
	var cases wrapperCases
	decodePage(t, readParityFile(t, path), &cases)
	return cases
}

func readParityFile(t *testing.T, path string) []byte {
	t.Helper()
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read fixture or generated page: %q", err)
	}
	return body
}

func decodePage(t *testing.T, output []byte, page any) {
	t.Helper()
	if err := json.Unmarshal(output, page); err != nil {
		t.Fatalf("decode page: %q, stdout %q", err, output)
	}
}

// Objects may contain additional fields. Arrays must have the expected length and order.
func checkFields(t *testing.T, field string, actual, expected any) {
	t.Helper()
	switch want := expected.(type) {
	case map[string]any:
		got, ok := actual.(map[string]any)
		if !ok {
			t.Fatalf("%q is not a JSON object", field)
		}
		for key, value := range want {
			item, exists := got[key]
			if !exists {
				t.Fatalf("%q is missing field %q", field, key)
			}
			checkFields(t, field+"."+key, item, value)
		}
	case []any:
		got, ok := actual.([]any)
		if !ok || len(got) != len(want) {
			t.Fatalf("%q must contain %d entries", field, len(want))
		}
		for i, value := range want {
			checkFields(t, fmt.Sprintf("%s[%d]", field, i), got[i], value)
		}
	default:
		if !reflect.DeepEqual(actual, expected) {
			t.Fatalf("%q: got %q, want %q", field, fmt.Sprint(actual), fmt.Sprint(expected))
		}
	}
}

func setupWrapperFixture(t *testing.T) (wrapperCases, string) {
	t.Helper()
	cases := loadWrapperCases(t)
	repo := filepath.Join(t.TempDir(), "wrapper-pages")
	if err := os.Mkdir(repo, 0o755); err != nil {
		t.Fatalf("create project: %q", err)
	}
	for name, content := range cases.Files {
		if err := os.WriteFile(filepath.Join(repo, name), []byte(content), 0o644); err != nil {
			t.Fatalf("write fixture: %q", err)
		}
	}
	return cases, repo
}

func successfulCLI(t *testing.T, binary, repo string, env []string, verb Verb, args ...string) []byte {
	t.Helper()
	stdout, stderr, status := runCLI(t, binary, repo, env, verb, args)
	if status != 0 {
		t.Fatalf("CLI exit %d for %q: %q", status, verb, stderr)
	}
	return stdout
}

func TestMixedNamedContextKeepsDeliveryHistory(t *testing.T) {
	binary := parityBinary(t)
	cases, repo := setupWrapperFixture(t)
	env := parityEnvironment(t)
	client := connectParity(t, binary, repo, env)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	named := cases.Context
	flags := []string{"--session", named.Session, "--dedupe"}
	mixed := append([]string{named.Good, named.Gone}, flags...)
	stdout, stderr, status := runCLI(t, binary, repo, env, VerbContext, mixed)
	output, err := client.Context(ctx, mixed...)
	failure := checkFailure(t, output, err, stdout, stderr, status)
	if failure.Code != 1 {
		t.Fatalf("mixed context exit %d, want 1", failure.Code)
	}
	full, missing, ok := bytes.Cut([]byte(failure.Output), []byte("\n"))
	if !ok || !bytes.HasSuffix(missing, []byte("\n")) {
		t.Fatalf("expected two JSON lines, got %q", failure.Output)
	}
	var fullPage, missingPage struct {
		Verb, Target string
		Chunks       []map[string]any
	}
	decodePage(t, full, &fullPage)
	decodePage(t, missing, &missingPage)
	if missingPage.Verb != "context" || missingPage.Target != named.Gone || missingPage.Chunks == nil || len(missingPage.Chunks) != 0 {
		t.Fatalf("unexpected missing target page: %q", missing)
	}
	if fullPage.Verb != "context" || fullPage.Target != named.Target || len(fullPage.Chunks) != 1 {
		t.Fatalf("unexpected full target page: %q", full)
	}
	chunk := fullPage.Chunks[0]
	checkFields(t, "full chunk", chunk, map[string]any{"role": "source", "target": named.Target, "ref": false, "content": named.Content})
	chunkID, ok := chunk["id"].(string)
	if !ok || chunkID == "" {
		t.Fatalf("missing chunk ID in %q", full)
	}
	single := append([]string{named.Good}, flags...)
	repeat, err := client.Context(ctx, single...)
	if err != nil {
		t.Fatalf("repeat named context: %q", err)
	}
	cliRepeat := successfulCLI(t, binary, repo, env, VerbContext, append([]string{named.Good, named.Good}, flags...)...)
	fullLine := append(bytes.Clone(full), '\n')
	checkBytes(t, "batched context", append(bytes.Clone(fullLine), repeat...), cliRepeat)
	var referencePage map[string]any
	decodePage(t, repeat, &referencePage)
	checkFields(t, "reference page", referencePage, map[string]any{
		"target": named.Target, "chunks": []any{map[string]any{"id": chunkID, "ref": true}},
	})
	if _, exists := referencePage["chunks"].([]any)[0].(map[string]any)["content"]; exists {
		t.Fatal("deduplicated chunk still has content")
	}
	fresh := connectParity(t, binary, repo, env)
	freshOutput, err := fresh.Context(ctx, single...)
	if err != nil {
		t.Fatalf("fresh named context: %q", err)
	}
	checkBytes(t, "fresh context stdout", freshOutput, successfulCLI(t, binary, repo, env, VerbContext, single...))
	checkBytes(t, "fresh context full body", freshOutput, fullLine)
}

func TestRealWrapperPages(t *testing.T) {
	binary := parityBinary(t)
	cases, repo := setupWrapperFixture(t)
	env := parityEnvironment(t)
	client := connectParity(t, binary, repo, env)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	page := func(t *testing.T, verb Verb, args ...string) map[string]any {
		t.Helper()
		cli := successfulCLI(t, binary, repo, env, verb, args...)
		output, err := client.Call(ctx, verb, args...)
		if err != nil {
			t.Fatalf("engine call %q failed: %q", verb, err)
		}
		checkBytes(t, "stdout", output, cli)
		var decoded map[string]any
		decodePage(t, output, &decoded)
		return decoded
	}
	t.Run("byte spans", func(t *testing.T) {
		for _, test := range cases.Reads {
			read := page(t, VerbRead, test.Target)
			disk := readParityFile(t, filepath.Join(repo, test.File))
			lines := bytes.Split(disk, []byte("\n"))
			first, last := test.Lines[0], test.Lines[1]
			start := 0
			for _, line := range lines[:first-1] {
				start += len(line) + 1
			}
			selected := bytes.TrimSuffix(bytes.Join(lines[first-1:last], []byte("\n")), []byte("\r"))
			end := start + len(selected)
			checkFields(t, "read page", read, map[string]any{"records": []any{map[string]any{
				"id": test.Target, "path": test.File, "lines": []any{float64(first), float64(last)},
				"span": []any{float64(start), float64(end)}, "source_utf8": string(disk[start:end]),
			}}})
		}
	})
	t.Run("truncated graphs", func(t *testing.T) {
		for _, test := range cases.Graph {
			checkFields(t, "graph page", page(t, test.Verb, test.Args...), test.Expected)
		}
	})
	t.Run("cursor", func(t *testing.T) {
		args := []string{cases.Outline.Target, "--limit", "1"}
		first := page(t, VerbMap, args...)
		checkFields(t, "first page", first, map[string]any{
			"records": []any{map[string]any{"id": cases.Outline.IDs[0]}}, "more": float64(1),
		})
		cursor, ok := first["next"].(string)
		if !ok || cursor == "" {
			t.Fatal("first page did not return a usable cursor")
		}
		second := page(t, VerbMap, append(args, "--cursor", cursor)...)
		checkFields(t, "last page", second, map[string]any{
			"records": []any{map[string]any{"id": cases.Outline.IDs[1]}}, "more": float64(0), "next": nil,
		})
	})
	t.Run("pack to file", func(t *testing.T) {
		target := filepath.Join(t.TempDir(), "pack.json")
		output, err := client.Pack(ctx, "--output", target)
		if err != nil {
			t.Fatalf("pack to file: %q", err)
		}
		checkBytes(t, "pack stdout", output, nil)
		saved := readParityFile(t, target)
		checkBytes(t, "saved pack", saved, successfulCLI(t, binary, repo, env, VerbPack))
		var report map[string]any
		decodePage(t, saved, &report)
		checkFields(t, "pack report", report, map[string]any{"files": float64(len(cases.Files))})
	})
	t.Run("extraction markers", func(t *testing.T) {
		// Extend a disposable file past the per-file cap so the index skips reading it.
		path := filepath.Join(repo, cases.Extraction.OversizedFile)
		if err := os.WriteFile(path, nil, 0o644); err != nil {
			t.Fatalf("create oversized fixture: %q", err)
		}
		if err := os.Truncate(path, cases.Extraction.Size); err != nil {
			t.Fatalf("size oversized fixture: %q", err)
		}
		for _, test := range []pageCase{cases.Extraction.Partial, cases.Extraction.Opaque} {
			checkFields(t, "extraction page", page(t, test.Verb, test.Args...), test.Expected)
		}
		refused := cases.Extraction.RefusedRead
		stdout, stderr, status := runCLI(t, binary, repo, env, refused.Verb, refused.Args)
		output, err := client.Call(ctx, refused.Verb, refused.Args...)
		failure := checkFailure(t, output, err, stdout, stderr, status)
		if failure.Code != 1 {
			t.Fatalf("refused read exit %d, want 1", failure.Code)
		}
		var failurePage map[string]any
		decodePage(t, []byte(failure.Output), &failurePage)
		checkFields(t, "refused read page", failurePage, refused.Expected)
	})
}
