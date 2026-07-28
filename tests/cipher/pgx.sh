#!/usr/bin/env bash
# pgx over TLS against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$CIPHER_WORK/pgx"
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

func main() {
	url := fmt.Sprintf(
		"postgres://%s:%s@%s:%s/%s?sslmode=require",
		os.Getenv("PGPROX_USER"), os.Getenv("PGPROX_TOKEN"),
		os.Getenv("PGPROX_HOST"), os.Getenv("PGPROX_PORT"), os.Getenv("PGPROX_DB"),
	)
	cfg, err := pgx.ParseConfig(url)
	if err != nil {
		fmt.Fprintln(os.Stderr, "parse:", err)
		os.Exit(1)
	}
	// The stack's certificate is self-signed and made at start, so there is
	// nothing to verify it against. sslmode=require already means encrypt
	// without verifying; this is explicit so a future pgx default cannot
	// quietly turn the probe into a certificate test.
	cfg.TLSConfig = &tls.Config{InsecureSkipVerify: true}

	ctx := context.Background()
	conn, err := pgx.ConnectConfig(ctx, cfg)
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
	fmt.Println("pgx: connected")
}
GO

cd "$WORK"
if [[ ! -f go.mod ]]; then
  go mod init pgxcipher >/dev/null
  go get github.com/jackc/pgx/v5 >/dev/null 2>&1
fi
go mod tidy >/dev/null 2>&1
go run .
