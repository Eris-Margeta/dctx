// SPDX-License-Identifier: Apache-2.0

// Package dctx transports CLI arguments and opaque engine output over dctx rpc.
// It never downloads an executable, inspects operation pages, or retries work.
// Each Client owns one engine subprocess rooted in a project directory.
package dctx

//go:generate go run ./internal/generate
