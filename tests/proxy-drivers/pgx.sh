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
	"strings"

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

	// PGPROX_DEPTH_UNNAMED_STATEMENT. `M20.6`. `QueryExecModeExec` sends the
	// one-shot form: `Parse` under the empty name, then `Bind` and `Execute`
	// on it. That statement is replaced by the next `Parse` of it and does not
	// survive a `Close`, which is what makes it different from a named one and
	// what the proxy used to erase by rewriting it to `pgprox_<hash>`.
	//
	// More than once, with different SQL, because one round would pass under
	// either behaviour. Repeating it is what would accumulate a permanent
	// server-side statement per distinct query if the rewrite came back.
	unnamedCfg, err := pgx.ParseConfig(url)
	if err != nil {
		die("parse for unnamed", err)
	}
	unnamedCfg.TLSConfig = &tls.Config{InsecureSkipVerify: true}
	unnamedCfg.DefaultQueryExecMode = pgx.QueryExecModeExec
	unnamed, err := pgx.ConnectConfig(ctx, unnamedCfg)
	if err != nil {
		die("connect for unnamed", err)
	}
	defer unnamed.Close(ctx)
	// Inside a transaction, so all of it lands on one upstream connection and
	// the count below is about the connection these ran on. Outside one the
	// pool is free to move the session between statements, which would make
	// the assertion a question about routing.
	utx, err := unnamed.Begin(ctx)
	if err != nil {
		die("begin for unnamed", err)
	}
	for i := 0; i < 3; i++ {
		if err := utx.QueryRow(ctx,
			fmt.Sprintf("SELECT $1::int + %d /* unnamed_probe */", i), 7).Scan(&n); err != nil {
			die("unnamed statement", err)
		}
		if int(n) != 7+i {
			fmt.Fprintf(os.Stderr, "pgx: unnamed statement gave %d\n", n)
			os.Exit(1)
		}
	}

	// The assertion that can tell the two behaviours apart. Both produce a
	// working sequence, so the queries passing proves nothing on its own: what
	// separates the unnamed statement from a named one is that the server does
	// not keep it. Rewriting it to `pgprox_<hash>` left one behind per distinct
	// one-shot query.
	//
	// Matched on the marker in the SQL rather than on the name prefix, because
	// this connection legitimately holds named statements from the rotation
	// case above and from whoever borrowed it before. Counting every
	// `pgprox_%` counted those too, and said four when it meant zero.
	//
	// The pattern is split so this statement does not match itself. Whole, its
	// own text carries the marker, so under the old behaviour it counted the
	// query doing the counting and reported four where three was the truth.
	if err := utx.QueryRow(ctx,
		"SELECT count(*) FROM pg_prepared_statements WHERE statement LIKE '%unnamed' || '_probe%'").Scan(&n); err != nil {
		die("counting prepared statements", err)
	}
	if n != 0 {
		fmt.Fprintf(os.Stderr,
			"pgx: %d one-shot queries were left prepared on the server\n", n)
		os.Exit(1)
	}
	if err := utx.Commit(ctx); err != nil {
		die("commit for unnamed", err)
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

	// PGPROX_DEPTH_REFUSED_AT_CONNECT. `M20.8`, through a second driver,
	// because a client that cannot start is the one case where every driver
	// reports differently: psql prints libpq's message, and pgx surfaces the
	// `ErrorResponse` as a connect error. What has to survive both is the
	// reason.
	replCfg, err := pgx.ParseConfig(url + "&replication=database")
	if err != nil {
		die("parse for replication", err)
	}
	replCfg.TLSConfig = &tls.Config{InsecureSkipVerify: true}
	if _, err := pgx.ConnectConfig(ctx, replCfg); err == nil {
		fmt.Fprintln(os.Stderr, "pgx: a replication connection was accepted")
		os.Exit(1)
	} else if !strings.Contains(err.Error(), "replication connections are not proxied") {
		die("a replication connection was refused for the wrong reason", err)
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
