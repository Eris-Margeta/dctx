// SPDX-License-Identifier: Apache-2.0

package dctx

import (
	"crypto/sha256"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
)

func TestGeneratedSchemaAndEngineVersion(t *testing.T) {
	schema, err := os.ReadFile("../../docs/protocol/dctx-rpc-v1.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	if fmt.Sprintf("%x", sha256.Sum256(schema)) != schemaSHA256 {
		t.Fatal("RPC schema changed: run go generate in packages/go")
	}
	version, err := os.ReadFile("../../VERSION")
	if err != nil {
		t.Fatal(err)
	}
	if strings.TrimSpace(string(version)) != Version {
		t.Fatal("Go/engine release mismatch: run go generate")
	}
	manifest, err := os.ReadFile("../../crates/dctx-cli/Cargo.toml")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(manifest), "\nversion = \""+Version+"\"\n") {
		t.Fatal("Go/CLI Cargo version mismatch")
	}
	// Compile-time host API contract for every generated operation.
	methods := reflect.TypeOf((*Client)(nil))
	for _, name := range []string{"Pack", "Context", "Map", "Find", "Read", "Skeleton", "Explain", "Neighbors", "Grep", "Hubs", "Path", "Languages", "CacheStatus"} {
		if _, ok := methods.MethodByName(name); !ok {
			t.Errorf("missing operation %s", name)
		}
	}
}

func TestWrapperBoundary(t *testing.T) {
	// New dependencies or filesystem capabilities require an explicit boundary review.
	allowedImports := map[string]bool{}
	for _, path := range []string{"bufio", "bytes", "context", "encoding/json", "errors", "fmt", "io", "os", "os/exec", "path/filepath", "regexp", "runtime", "strconv", "sync/atomic", "time", "unicode/utf8"} {
		allowedImports[path] = true
	}
	allowedOS := map[string]bool{}
	for _, name := range []string{"File", "LookupEnv", "Pipe", "Stat", "Stderr", "UserCacheDir", "UserHomeDir"} {
		allowedOS[name] = true
	}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range files {
		if strings.HasSuffix(path, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(token.NewFileSet(), path, nil, 0)
		if err != nil {
			t.Fatal(err)
		}
		for _, imp := range file.Imports {
			value, err := strconv.Unquote(imp.Path.Value)
			if err != nil {
				t.Fatal(err)
			}
			if imp.Name != nil || !allowedImports[value] {
				t.Errorf("%s: unreviewed runtime import %s", path, imp.Path.Value)
			}
		}
		ast.Inspect(file, func(n ast.Node) bool {
			if call, ok := n.(*ast.CallExpr); ok {
				if selector, ok := call.Fun.(*ast.SelectorExpr); ok {
					if ident, ok := selector.X.(*ast.Ident); ok && ident.Name == "exec" {
						if selector.Sel.Name != "Command" || len(call.Args) != 2 {
							t.Errorf("%s: subprocesses must only start the engine RPC", path)
						} else if arg, ok := call.Args[1].(*ast.BasicLit); !ok || arg.Value != `"rpc"` {
							t.Errorf("%s: subprocess command must be rpc", path)
						}
					}
				}
			}
			selector, ok := n.(*ast.SelectorExpr)
			if !ok {
				return true
			}
			ident, ok := selector.X.(*ast.Ident)
			if ok && ident.Name == "os" && !allowedOS[selector.Sel.Name] {
				t.Errorf("%s: unreviewed filesystem capability os.%s", path, selector.Sel.Name)
			}
			if ok && ident.Name == "filepath" && selector.Sel.Name != "Abs" && selector.Sel.Name != "Join" && selector.Sel.Name != "IsAbs" {
				t.Errorf("%s: unreviewed filesystem capability filepath.%s", path, selector.Sel.Name)
			}
			return true
		})
	}
}
