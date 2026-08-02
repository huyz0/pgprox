#!/usr/bin/env bash
# pgx against the proxy.
#
# pgx uses named prepared statements by default, which is the behaviour that
# forces prepared-statement mapping in the pool. See ADR 0011.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$PROBE_WORK/pgx"
mkdir -p "$WORK"
cat > "$WORK/main.go" <<'GO'
package main

import (
	"context"
	"crypto/tls"
	"fmt"
	"os"

	"github.com/jackc/pgx/v5"
)

func die(what string, err error) {
	fmt.Fprintln(os.Stderr, "pgx:", what+":", err)
	os.Exit(1)
}

func main() {
	url := fmt.Sprintf(
		"postgres://%s:%s@%s:%s/%s?sslmode=require",
		os.Getenv("PGPROX_USER"), os.Getenv("PGPROX_TOKEN"),
		os.Getenv("PGPROX_HOST"), os.Getenv("PGPROX_PORT"), os.Getenv("PGPROX_DB"),
	)
	cfg, err := pgx.ParseConfig(url)
	if err != nil {
		die("parse", err)
	}
	// The stack's certificate is self-signed and made at start, so there is
	// nothing to verify it against. sslmode=require already means encrypt
	// without verifying; this is explicit so a future pgx default cannot
	// quietly turn the probe into a certificate test.
	cfg.TLSConfig = &tls.Config{InsecureSkipVerify: true}

	ctx := context.Background()
	conn, err := pgx.ConnectConfig(ctx, cfg)
	if err != nil {
		die("connect", err)
	}
	defer conn.Close(ctx)

	var n int32
	if err := conn.QueryRow(ctx, "SELECT 1").Scan(&n); err != nil {
		die("query", err)
	}

	// A bound parameter, which is the extended protocol.
	if err := conn.QueryRow(ctx, "SELECT $1::int + 1", 41).Scan(&n); err != nil {
		die("parameter", err)
	}
	if n != 42 {
		fmt.Fprintf(os.Stderr, "pgx: parameter gave %d\n", n)
		os.Exit(1)
	}

	// PGPROX_DEPTH_PREPARED_REUSE. pgx caches by SQL text, so the same
	// statement repeatedly reuses one server-side prepare, and the second use
	// sends Bind alone naming what the first parsed.
	for i := 0; i < 5; i++ {
		if err := conn.QueryRow(ctx, "SELECT $1::int", 7).Scan(&n); err != nil {
			die("prepared reuse", err)
		}
	}

	// PGPROX_DEPTH_STATEMENT_ROTATION. `M20.1`, from the driver that produces
	// it in the wild: a protocol `Close` of a prepared statement, then the
	// same SQL again.
	//
	// The proxy rewrites the `Close` to its own global name and forwards it,
	// so the server really does deallocate the statement. Until `M20.1`
	// neither of its two maps heard about that, and the connection went on
	// claiming to hold what it had just dropped, so this second `Bind` named
	// something that was gone: `26000 prepared statement "pgprox_..." does
	// not exist`. It outlived the session that caused it, because the
	// connection went back to the pool still mis-recorded.
	//
	// An explicitly named statement, deallocated by name. `DeallocateAll` is
	// not this: pgx sends `DEALLOCATE ALL` as SQL for it, which the proxy
	// already handles through `deallocates_everything` since `M15.3`, so a
	// probe built on it passes with `M20.1` reverted. Checked, not assumed.
	if _, err := conn.Prepare(ctx, "rotating", "SELECT $1::int + 11"); err != nil {
		die("prepare by name", err)
	}
	if err := conn.QueryRow(ctx, "rotating", 7).Scan(&n); err != nil {
		die("named statement", err)
	}
	if err := conn.Deallocate(ctx, "rotating"); err != nil {
		die("deallocate by name", err)
	}
	if _, err := conn.Prepare(ctx, "rotating", "SELECT $1::int + 11"); err != nil {
		die("re-prepare by name", err)
	}
	if err := conn.QueryRow(ctx, "rotating", 7).Scan(&n); err != nil {
		die("re-prepare after a protocol Close", err)
	}
	if n != 18 {
		fmt.Fprintf(os.Stderr, "pgx: re-prepare gave %d\n", n)
		os.Exit(1)
	}

	// PGPROX_DEPTH_LARGE_RESULT.
	rows, err := conn.Query(ctx, "SELECT generate_series(1, 5000)")
	if err != nil {
		die("large result", err)
	}
	count := 0
	for rows.Next() {
		count++
	}
	rows.Close()
	if count != 5000 {
		fmt.Fprintf(os.Stderr, "pgx: large result gave %d rows\n", count)
		os.Exit(1)
	}

	// A transaction, which is what the pool releases on.
	tx, err := conn.Begin(ctx)
	if err != nil {
		die("begin", err)
	}
	if err := tx.QueryRow(ctx, "SELECT 2").Scan(&n); err != nil {
		die("in transaction", err)
	}
	if err := tx.Commit(ctx); err != nil {
		die("commit", err)
	}

	// An error, and a statement after it. A session the proxy left
	// mid-transaction would fail the second one.
	if err := conn.QueryRow(ctx, "SELECT no_such_column_xyz").Scan(&n); err == nil {
		fmt.Fprintln(os.Stderr, "pgx: a bad column succeeded")
		os.Exit(1)
	}
	if err := conn.QueryRow(ctx, "SELECT 3").Scan(&n); err != nil {
		die("after an error", err)
	}

	fmt.Println("pgx: ok")
}
GO

cd "$WORK"
if [[ ! -f go.mod ]]; then
  go mod init pgxproxy >/dev/null
  go get github.com/jackc/pgx/v5 >/dev/null 2>&1
fi
go mod tidy >/dev/null 2>&1
go run .
