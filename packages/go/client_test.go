// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestMain(m *testing.M) {
	if os.Getenv("DCTX_GO_HELPER") == "1" {
		helperServer()
		os.Exit(0)
	}
	os.Exit(m.Run())
}

// A real subprocess fixture exercises pipes on every supported host, without a shell.
func helperServer() {
	if len(os.Args) != 2 || os.Args[1] != "rpc" {
		os.Exit(80)
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 2*1024*1024)
	for scanner.Scan() {
		var request Request
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			os.Exit(81)
		}
		reply := map[string]any{"id": request.ID, "engine_version": Version, "future": true}
		if request.Verb == VerbHello {
			reply["result"] = map[string]any{"protocol_version": "1.7.2+metadata", "verbs": []string{"hello", "languages", "future_verb"}, "future": 1}
		} else {
			if len(request.Args) > 0 && request.Args[0] == "block" {
				if err := os.WriteFile(os.Getenv("DCTX_GO_READY"), []byte("ready"), 0o600); err != nil {
					os.Exit(82)
				}
				time.Sleep(time.Hour)
			}
			reply["result"] = map[string]any{"output": strings.Join(request.Args, "\x00") + "\n\n", "future": false}
		}
		scenario := os.Getenv("DCTX_GO_SCENARIO")
		if request.Verb == VerbHello {
			switch scenario {
			case "engine":
				reply["engine_version"] = "0.0.0"
			case "major":
				reply["result"] = map[string]any{"protocol_version": "2.0.0", "verbs": []string{}}
			case "protocol":
				reply["result"] = map[string]any{"protocol_version": "1.nope.0", "verbs": []string{}}
			case "verbs":
				reply["result"] = map[string]any{"protocol_version": "1.0.0", "verbs": []int{1}}
			case "null-verbs":
				reply["result"] = map[string]any{"protocol_version": "1.0.0", "verbs": nil}
			case "null-verb":
				reply["result"] = map[string]any{"protocol_version": "1.0.0", "verbs": []any{nil}}
			case "missing-protocol":
				reply["result"] = map[string]any{"verbs": []string{}}
			case "id":
				reply["id"] = "wrong"
			case "missing-id":
				delete(reply, "id")
			case "null-id":
				reply["id"] = nil
			case "missing-engine":
				delete(reply, "engine_version")
			case "both":
				reply["error"] = map[string]any{"code": 2, "message": "refusal", "hint": "hint", "output": ""}
			case "neither":
				delete(reply, "result")
			case "null-result":
				reply["result"] = nil
			case "malformed":
				fmt.Fprintln(os.Stdout, "{")
				return
			case "unterminated":
				fmt.Fprint(os.Stdout, `{}`)
				return
			case "utf8":
				os.Stdout.Write([]byte{'"', 0xff, '"', '\n'})
				return
			case "exit":
				os.Exit(3)
			}
		} else {
			switch scenario {
			case "capture":
				reply["result"] = map[string]any{"output": scanner.Text() + "\n"}
			case "operation-engine":
				reply["engine_version"] = "0.0.0"
			case "output-type":
				reply["result"] = map[string]any{"output": 7}
			case "output-null":
				reply["result"] = map[string]any{"output": nil}
			case "output-missing":
				reply["result"] = map[string]any{}
			case "error", "error-code", "error-missing", "error-null", "error-type":
				delete(reply, "result")
				failure := map[string]any{"code": 1, "message": "unchanged diagnostic", "hint": "unchanged hint", "output": "gone\n\n"}
				switch scenario {
				case "error-code":
					failure["code"] = 4
				case "error-missing":
					delete(failure, "output")
				case "error-null":
					failure["message"] = nil
				case "error-type":
					failure["hint"] = false
				}
				reply["error"] = failure
			}
		}
		if err := json.NewEncoder(os.Stdout).Encode(reply); err != nil {
			os.Exit(83)
		}
	}
}

func helperOptions(t *testing.T, scenario string) Options {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return Options{Binary: binary, Env: append(os.Environ(), "DCTX_GO_HELPER=1", "DCTX_GO_SCENARIO="+scenario)}
}

func helperClient(t *testing.T, scenario string) *Client {
	t.Helper()
	client, err := New(context.Background(), t.TempDir(), helperOptions(t, scenario))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := client.Close(); err != nil {
			t.Error(err)
		}
	})
	return client
}

func TestHandshakeRefusesInvalidEnvelopes(t *testing.T) {
	for _, scenario := range []string{"engine", "major", "protocol", "verbs", "null-verbs", "null-verb", "missing-protocol", "id", "missing-id", "null-id", "missing-engine", "both", "neither", "null-result", "malformed", "unterminated", "utf8", "exit"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			client, err := New(ctx, t.TempDir(), helperOptions(t, scenario))
			if client != nil {
				client.Terminate()
				t.Fatal("invalid handshake accepted")
			}
			if err == nil {
				t.Fatal("missing handshake error")
			}
			if scenario == "engine" || scenario == "major" || scenario == "protocol" {
				var mismatch *VersionMismatchError
				if !errors.As(err, &mismatch) {
					t.Fatalf("expected version mismatch, got %v", err)
				}
			}
		})
	}
}

func TestOpaqueArgumentsOutputAndConcurrentCalls(t *testing.T) {
	client := helperClient(t, "")
	args := []string{" spaced ' $() \" ; ", "--budget", "3", "line\nnext", "🙂", strings.Repeat("界", 40_000)}
	output, err := client.Find(context.Background(), args...)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(output, []byte(strings.Join(args, "\x00")+"\n\n")) {
		t.Fatal("argument or output bytes changed")
	}
	var wg sync.WaitGroup
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			arg := fmt.Sprint(i)
			output, err := client.Languages(context.Background(), arg)
			if err != nil || string(output) != arg+"\n\n" {
				t.Errorf("correlation: %q, %v", output, err)
			}
		}(i)
	}
	wg.Wait()
	if err := client.Close(); err != nil {
		t.Fatal(err)
	}
	if err := client.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := client.Find(context.Background(), "after-close"); err == nil {
		t.Fatal("closed client accepted work")
	}
}

func TestEngineErrorPreservesFieldsAndSession(t *testing.T) {
	client := helperClient(t, "error")
	_, err := client.Read(context.Background(), "anything")
	var failure *EngineError
	if !errors.As(err, &failure) {
		t.Fatalf("expected engine error, got %v", err)
	}
	if failure.Code != 1 || failure.Message != "unchanged diagnostic" || failure.Error() != failure.Message || failure.Hint != "unchanged hint" || failure.Output != "gone\n\n" {
		t.Fatalf("modified error: %+v", failure)
	}
	if _, err := client.Hello(context.Background()); err != nil {
		t.Fatal("engine refusal poisoned session:", err)
	}
}

func TestInvalidOperationResponseEndsSession(t *testing.T) {
	for _, scenario := range []string{"operation-engine", "output-type", "output-null", "output-missing", "error-code", "error-missing", "error-null", "error-type"} {
		t.Run(scenario, func(t *testing.T) {
			client := helperClient(t, scenario)
			if _, err := client.Find(context.Background()); err == nil {
				t.Fatal("invalid reply accepted")
			}
			if _, err := client.Hello(context.Background()); err == nil {
				t.Fatal("invalid session reused")
			}
			select {
			case <-client.done:
			default:
				t.Fatal("failed engine not reaped")
			}
		})
	}
}

func TestInvalidRequestsDoNotPoisonSession(t *testing.T) {
	client := helperClient(t, "")
	for _, arg := range []string{"bad\n\x1b\"\xff", "\xed\xa0\x80"} {
		if _, err := client.Find(context.Background(), arg); err == nil || err.Error() != "RPC arguments must be UTF-8" {
			t.Fatalf("expected local invalid UTF-8 refusal, got %q", err)
		}
	}
	if _, err := client.Call(context.Background(), VerbHello); err == nil {
		t.Fatal("Call accepted hello")
	}
	if _, err := client.Languages(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestCompactRequestBytesAndFrameBoundary(t *testing.T) {
	client := helperClient(t, "capture")
	ctx := context.Background()
	ordinary, err := client.Find(ctx, "--limit", "2", "two words")
	if expected := `{"args":["--limit","2","two words"],"id":"2","verb":"find"}` + "\n"; err != nil || string(ordinary) != expected {
		t.Fatalf("ordinary request = %q, error = %q", ordinary, err)
	}

	arg := "\n\x1b\"\\λ&<>\u2028\u2029" + `\u2028`
	prefix := `{"args":["--help","\n\u001b\"\\` + "λ&<>\u2028\u2029" + `\\u2028`
	suffix := `"],"id":"3","verb":"languages"}` + "\n"
	padding := strings.Repeat("x", 1_048_576-len(prefix)-len(suffix)+1)
	exact, err := client.Languages(ctx, "--help", arg+padding)
	if err != nil {
		t.Fatalf("exact limit request refused: %q", err)
	}
	if len(exact) != 1_048_577 || string(exact) != prefix+padding+suffix {
		t.Fatalf("exact limit request wire bytes changed (got %d bytes including LF)", len(exact))
	}
	if _, err := client.Languages(ctx, "--help", arg+padding+"x"); err == nil ||
		err.Error() != "RPC request exceeds the engine's 1,048,576-byte frame limit" {
		t.Fatalf("expected local frame limit refusal, got %q", err)
	}
	reused, err := client.Languages(ctx)
	if expected := `{"id":"5","verb":"languages"}` + "\n"; err != nil || string(reused) != expected {
		t.Fatalf("request after refusal = %q, error = %q", reused, err)
	}
}

func TestTransportDiagnosticQuotesCause(t *testing.T) {
	cause := errors.New("bad\n\x1b\"")
	err := &TransportError{cause}
	if got, want := err.Error(), `dctx transport: "bad\n\x1b\""`; got != want {
		t.Errorf("diagnostic = %q, want %q", got, want)
	}
}

func waitReady(t *testing.T, path string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(path); err == nil {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("helper did not receive active operation")
}

func TestCancellationAndTermination(t *testing.T) {
	for _, terminate := range []bool{false, true} {
		t.Run(fmt.Sprint("terminate=", terminate), func(t *testing.T) {
			ready := filepath.Join(t.TempDir(), "ready")
			options := helperOptions(t, "")
			options.Env = append(options.Env, "DCTX_GO_READY="+ready)
			client, err := New(context.Background(), t.TempDir(), options)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(client.Terminate)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			done := make(chan error, 1)
			go func() { _, err := client.Find(ctx, "block"); done <- err }()
			waitReady(t, ready)
			queuedCtx, queuedCancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
			defer queuedCancel()
			if _, err := client.Find(queuedCtx, "never-sent"); !errors.Is(err, context.DeadlineExceeded) {
				t.Fatal("queued cancellation:", err)
			}
			if terminate {
				client.Terminate()
			} else {
				cancel()
			}
			select {
			case err := <-done:
				if err == nil {
					t.Fatal("aborted call succeeded")
				}
				if !terminate && !errors.Is(err, context.Canceled) {
					t.Fatal("lost cancellation:", err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("aborted call did not return")
			}
			if _, err := client.Hello(context.Background()); err == nil {
				t.Fatal("aborted session reused")
			}
			if err := client.Close(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestCanceledStartupAndRequiredProject(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := New(ctx, t.TempDir(), helperOptions(t, "")); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if _, err := New(context.Background(), "", helperOptions(t, "")); err == nil {
		t.Fatal("missing project accepted")
	}
	if _, err := New(context.Background(), filepath.Join(t.TempDir(), "missing"), helperOptions(t, "")); err == nil {
		t.Fatal("missing working directory accepted")
	}
}
