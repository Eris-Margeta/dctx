// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"regexp"
	"strconv"
	"sync/atomic"
	"time"
	"unicode/utf8"
)

// Options configures process transport. A nil Env inherits the caller's environment.
type Options struct {
	Binary string
	Env    []string
}

// TransportError indicates a failed process or invalid RPC envelope. Work is not retried.
type TransportError struct{ Cause error }

func (e *TransportError) Error() string { return "dctx transport: " + e.Cause.Error() }
func (e *TransportError) Unwrap() error { return e.Cause }

// VersionMismatchError indicates an incompatible engine release or RPC major.
type VersionMismatchError struct{ Expected, Actual string }

func (e *VersionMismatchError) Error() string {
	return fmt.Sprintf("dctx version mismatch: require %s, got %q. %s", e.Expected, e.Actual, InstallInstructions())
}

// Error returns the engine diagnostic unchanged. Branch on Code, not its text.
// Output holds exact CLI stdout emitted before the failure, including newlines.
func (e *EngineError) Error() string { return e.Message }

// Client owns one synchronous engine process. Concurrent calls are serialized.
// Canceling an active call terminates the session because RPC v1 has no in-band
// cancellation. Close drains active work; Terminate aborts it from any goroutine.
type Client struct {
	cmd      *exec.Cmd
	input    *os.File
	output   *os.File
	reader   *bufio.Reader
	gate     chan struct{}
	done     chan struct{}
	waitErr  error // published by closing done
	aborted  atomic.Bool
	closed   bool   // guarded by gate
	closeErr error  // guarded by gate
	nextID   uint64 // string IDs avoid numeric overflow on the wire
}

// New starts dctx rpc in project and completes the version-matched hello before
// returning. ctx controls startup and handshake; subsequent calls own their contexts.
// The caller must Close or Terminate the returned client.
func New(ctx context.Context, project string, options Options) (*Client, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if project == "" {
		return nil, fmt.Errorf("dctx project directory is required")
	}
	binary, err := ResolveBinary(options.Binary)
	if err != nil {
		return nil, err
	}
	inputRead, inputWrite, err := os.Pipe()
	if err != nil {
		return nil, &TransportError{err}
	}
	outputRead, outputWrite, err := os.Pipe()
	if err != nil {
		inputRead.Close()
		inputWrite.Close()
		return nil, &TransportError{err}
	}
	cmd := exec.Command(binary, "rpc")
	cmd.Dir, cmd.Env = project, options.Env
	cmd.Stdin, cmd.Stdout, cmd.Stderr = inputRead, outputWrite, os.Stderr
	err = cmd.Start()
	inputRead.Close()
	outputWrite.Close()
	if err != nil {
		inputWrite.Close()
		outputRead.Close()
		return nil, &TransportError{fmt.Errorf("start engine: %w; %s", err, InstallInstructions())}
	}
	c := &Client{cmd: cmd, input: inputWrite, output: outputRead, reader: bufio.NewReader(outputRead), gate: make(chan struct{}, 1), done: make(chan struct{})}
	c.gate <- struct{}{}
	// Own the pipe files ourselves: exec.Wait must not close stdout while a
	// response is still being read. stderr inherits an unbuffered OS descriptor.
	go func() { c.waitErr = cmd.Wait(); close(c.done) }()
	if _, err := c.Hello(ctx); err != nil {
		c.Terminate()
		return nil, err
	}
	return c, nil
}

func (c *Client) acquire(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-c.gate:
		if err := ctx.Err(); err != nil {
			c.release()
			return err
		}
		return nil
	}
}
func (c *Client) release() { c.gate <- struct{}{} }

// Terminate aborts active work, closes protocol pipes, and reaps the process.
// It is safe to call concurrently with Call, Close, or another Terminate.
func (c *Client) Terminate() {
	c.aborted.Store(true)
	_ = c.cmd.Process.Kill()
	_ = c.input.Close()
	_ = c.output.Close()
	<-c.done
}

// Close sends EOF after active work finishes, waits up to five seconds for
// shutdown, and then terminates if necessary. Repeated closes return the same error.
func (c *Client) Close() error {
	_ = c.acquire(context.Background())
	defer c.release()
	if c.closed {
		return c.closeErr
	}
	c.closed = true
	_ = c.input.Close()
	timer := time.NewTimer(5 * time.Second)
	defer timer.Stop()
	select {
	case <-c.done:
	case <-timer.C:
		c.Terminate()
	}
	_ = c.output.Close()
	if !c.aborted.Load() && c.waitErr != nil {
		c.closeErr = &TransportError{c.waitErr}
	}
	return c.closeErr
}

func decodeObject(data []byte, value any, required ...string) error {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	if fields == nil {
		return fmt.Errorf("expected an object")
	}
	for _, key := range required {
		v, exists := fields[key]
		if !exists || bytes.Equal(bytes.TrimSpace(v), []byte("null")) {
			return fmt.Errorf("missing or null %s", key)
		}
	}
	return json.Unmarshal(data, value)
}

func (c *Client) exchange(verb Verb, args []string) (json.RawMessage, error) {
	if c.closed || c.aborted.Load() {
		return nil, fmt.Errorf("dctx client is closed")
	}
	select {
	case <-c.done:
		return nil, &TransportError{fmt.Errorf("engine exited: %v", c.waitErr)}
	default:
	}
	if !utf8.ValidString(string(verb)) {
		return nil, fmt.Errorf("RPC verb must be UTF-8")
	}
	for _, arg := range args {
		if !utf8.ValidString(arg) {
			return nil, fmt.Errorf("RPC arguments must be UTF-8")
		}
	}
	if args == nil {
		args = []string{}
	}
	c.nextID++
	id, _ := json.Marshal(strconv.FormatUint(c.nextID, 10))
	frame, err := json.Marshal(Request{ID: id, Verb: verb, Args: args})
	if err != nil {
		return nil, err
	}
	if len(frame) > 1_048_576 {
		return nil, fmt.Errorf("RPC request exceeds the engine's 1,048,576-byte frame limit")
	}
	if _, err := c.input.Write(append(frame, '\n')); err != nil {
		return nil, &TransportError{err}
	}
	line, err := c.reader.ReadBytes('\n') // No scanner limit: engine output is never truncated.
	if err != nil {
		return nil, &TransportError{fmt.Errorf("read complete response: %w", err)}
	}
	if !utf8.Valid(line) {
		return nil, &TransportError{fmt.Errorf("response is not UTF-8")}
	}
	var response Response
	if err := decodeObject(line, &response, "id", "engine_version"); err != nil {
		return nil, &TransportError{err}
	}
	var actualID string
	if json.Unmarshal(response.ID, &actualID) != nil || actualID != strconv.FormatUint(c.nextID, 10) {
		return nil, &TransportError{fmt.Errorf("invalid response correlation id")}
	}
	if response.EngineVersion != Version {
		return nil, &VersionMismatchError{Expected: "engine " + Version, Actual: response.EngineVersion}
	}
	if (len(response.Result) == 0) == (len(response.Error) == 0) {
		return nil, &TransportError{fmt.Errorf("response must contain exactly one result or error")}
	}
	if len(response.Error) > 0 {
		var failure EngineError
		if err := decodeObject(response.Error, &failure, "code", "message", "hint", "output"); err != nil {
			return nil, &TransportError{err}
		}
		if failure.Code < 1 || failure.Code > 3 {
			return nil, &TransportError{fmt.Errorf("invalid engine error code")}
		}
		return nil, &failure
	}
	return response.Result, nil
}

// withExchange serializes transport and ties only active work to cancellation.
func (c *Client) withExchange(ctx context.Context, verb Verb, args []string, decode func(json.RawMessage) error) (err error) {
	if err := c.acquire(ctx); err != nil {
		return err
	}
	defer c.release()
	canceled := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { c.Terminate(); close(canceled) })
	defer func() {
		if !stop() {
			<-canceled
			err = ctx.Err()
		}
		var transport *TransportError
		var mismatch *VersionMismatchError
		if errors.As(err, &transport) || errors.As(err, &mismatch) {
			c.Terminate()
		}
	}()
	result, err := c.exchange(verb, args)
	if err != nil {
		return err
	}
	return decode(result)
}

const numericVersion = `(0|[1-9][0-9]*)`
const prereleaseIdentifier = `(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)`

var protocolVersion = regexp.MustCompile(`^1\.` + numericVersion + `\.` + numericVersion +
	`(-` + prereleaseIdentifier + `(\.` + prereleaseIdentifier + `)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$`)

// Hello returns protocol metadata. RPC major 1 and the exact engine
// release are required. Unknown envelope fields and additional verbs are tolerated.
func (c *Client) Hello(ctx context.Context) (Hello, error) {
	var hello Hello
	err := c.withExchange(ctx, VerbHello, nil, func(data json.RawMessage) error {
		if err := decodeObject(data, &hello, "protocol_version", "verbs"); err != nil {
			return &TransportError{err}
		}
		if !protocolVersion.MatchString(hello.ProtocolVersion) {
			return &VersionMismatchError{Expected: "RPC major 1", Actual: hello.ProtocolVersion}
		}
		for _, verb := range hello.Verbs {
			if verb == "" {
				return &TransportError{fmt.Errorf("invalid hello verb")}
			}
		}
		return nil
	})
	return hello, err
}

// Call passes CLI arguments to an operation and returns exact CLI stdout bytes.
// Use Hello for handshake metadata. Engine failures are *EngineError, preserving
// the original code, message, hint and output; no operation is automatically retried.
func (c *Client) Call(ctx context.Context, verb Verb, args ...string) ([]byte, error) {
	if verb == VerbHello {
		return nil, fmt.Errorf("use Hello for protocol metadata")
	}
	var output Output
	err := c.withExchange(ctx, verb, args, func(data json.RawMessage) error {
		if err := decodeObject(data, &output, "output"); err != nil {
			return &TransportError{err}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return []byte(output.Output), nil
}

var _ io.Closer = (*Client)(nil)
