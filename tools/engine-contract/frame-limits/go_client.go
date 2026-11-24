// SPDX-License-Identifier: Apache-2.0

package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/Eris-Margeta/dctx/packages/go"
)

func run() error {
	var config struct {
		Binary, Project string
		Env             map[string]string
		Cases           []struct {
			Name, Fill, Suffix string
			Repeat             int
		}
	}
	if err := json.NewDecoder(os.Stdin).Decode(&config); err != nil {
		return err
	}
	env := make([]string, 0, len(config.Env))
	for key, value := range config.Env {
		env = append(env, key+"="+value)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	client, err := dctx.New(ctx, config.Project, dctx.Options{Binary: config.Binary, Env: env})
	if err != nil {
		return err
	}
	defer client.Close()
	results := make([]*string, 0, len(config.Cases))
	for _, test := range config.Cases {
		args := []string{"--help"}
		if test.Fill != "" {
			args = append(args, strings.Repeat(test.Fill, test.Repeat)+test.Suffix)
		}
		output, err := client.Languages(ctx, args...)
		if err != nil {
			if err.Error() != "RPC request exceeds the engine's 1,048,576-byte frame limit" {
				return fmt.Errorf("case %q: %w", test.Name, err)
			}
			results = append(results, nil)
		} else {
			text := string(output)
			results = append(results, &text)
		}
	}
	if err := client.Close(); err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(results)
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintf(os.Stderr, "Go frame limits: %q\n", err.Error())
		os.Exit(1)
	}
}
