#!/usr/bin/env bash
# pgx against the conformance harness.
#
# pgx uses named prepared statements by default, which is the behaviour that
# forces prepared statement mapping in the pool. See ADR 0011.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_harness.sh"

start_harness

WORK="$CONFORMANCE_ROOT/target/pgx-check"
mkdir -p "$WORK"
cat > "$WORK/main.go" <<'GO'
package main

import (
	"context"
	"fmt"
	"os"

	"github.com/jackc/pgx/v5"
)

func main() {
	url := fmt.Sprintf(
		"postgres://postgres@127.0.0.1:%s/conformance?sslmode=disable",
		os.Getenv("PGPROX_HARNESS_PORT"),
	)
	ctx := context.Background()

	conn, err := pgx.Connect(ctx, url)
	if err != nil {
		fmt.Fprintln(os.Stderr, "connect:", err)
		os.Exit(1)
	}
	defer conn.Close(ctx)

	var n int32
	if err := conn.QueryRow(ctx, "SELECT 1").Scan(&n); err != nil {
		fmt.Fprintln(os.Stderr, "query:", err)
		os.Exit(1)
	}
	if n != 1 {
		fmt.Fprintf(os.Stderr, "expected 1, got %d\n", n)
		os.Exit(1)
	}

	// Again on the same connection, so a statement cached from the first round
	// is reused rather than re-prepared.
	if err := conn.QueryRow(ctx, "SELECT 1").Scan(&n); err != nil {
		fmt.Fprintln(os.Stderr, "second query:", err)
		os.Exit(1)
	}

	// PGPROX_DEPTH_PREPARED_REUSE: pgx caches by SQL text, so running the
	// same statement repeatedly reuses one server-side prepare.
	for i := 0; i < 5; i++ {
		if err := conn.QueryRow(ctx, "SELECT 1").Scan(&n); err != nil {
			fmt.Fprintln(os.Stderr, "prepared reuse:", err)
			os.Exit(1)
		}
	}

	// PGPROX_DEPTH_LARGE_RESULT: more than one TCP segment of rows.
	rows, err := conn.Query(ctx, "SELECT pgprox_large")
	if err != nil {
		fmt.Fprintln(os.Stderr, "large result:", err)
		os.Exit(1)
	}
	count := 0
	for rows.Next() {
		count++
	}
	rows.Close()
	if count < 2000 {
		fmt.Fprintf(os.Stderr, "large result gave %d rows\n", count)
		os.Exit(1)
	}

	fmt.Println("pgx: ok")
}
GO

cd "$WORK"
if [[ ! -f go.mod ]]; then
  go mod init pgxcheck >/dev/null
  go get github.com/jackc/pgx/v5 >/dev/null 2>&1
fi
go mod tidy >/dev/null 2>&1
go run .
