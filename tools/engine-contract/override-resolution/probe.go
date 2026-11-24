// SPDX-License-Identifier: Apache-2.0

package main

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/Eris-Margeta/dctx/packages/go"
)

func probe(project, binary string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	client, err := dctx.New(ctx, project, dctx.Options{Binary: binary})
	if err != nil {
		return err
	}
	output, err := client.Languages(ctx)
	closeErr := client.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	_, err = os.Stdout.Write(output)
	return err
}

func main() {
	if err := probe(os.Args[1], os.Args[2]); err != nil {
		fmt.Fprintf(os.Stderr, "%+q\n", err.Error())
		os.Exit(1)
	}
}
