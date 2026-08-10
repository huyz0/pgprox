#!/usr/bin/env bash
# Turns TLS on for the primary, using the certificate the `upstream-tls`
# service made before this container started.
#
# Runs from the postgres image's own init hook, numbered `05` so it lands
# before `10-init.sh`: both append to postgresql.conf and it costs nothing to
# keep the order the names imply.
#
# # Why the key is copied rather than pointed at
#
# Postgres refuses to start on a key it considers loosely held: it must be
# owned by the database user with no group or world access, or owned by root
# with no more than group read. The generator runs as root in a different
# container and cannot know this image's postgres uid, so the file it leaves in
# the shared volume is deliberately world-readable and deliberately not what the
# server opens. This script runs *as* postgres, so the copy it makes is owned by
# postgres, and `chmod 600` then satisfies the check by construction rather than
# by a uid somebody has to keep true.
#
# # Relative paths, because a replica inherits this file
#
# The replicas are `pg_basebackup` clones, so they get this postgresql.conf and
# the certificate beside it. `ssl_cert_file` is therefore relative, resolved
# against each server's own data directory: an absolute path would name the
# primary's `PGDATA`, which is not where the replica keeps its copy, and the
# replica would fail to start on a file that is right there under another name.
set -euo pipefail

CA_DIR=/upstream-tls

# The generator is a separate service and compose waits for it to exit before
# starting this container, so the files are here or something is wrong in a way
# worth saying out loud rather than starting without TLS.
for file in server.crt server.key; do
  if [ ! -s "$CA_DIR/$file" ]; then
    echo "no $CA_DIR/$file: the upstream-tls service did not leave one" >&2
    exit 1
  fi
done

cp "$CA_DIR/server.crt" "$PGDATA/server.crt"
cp "$CA_DIR/server.key" "$PGDATA/server.key"
chmod 600 "$PGDATA/server.key"
chmod 644 "$PGDATA/server.crt"

cat >> "$PGDATA/postgresql.conf" <<-CONF
	# TLS, so the proxy's own upstream connections have something to negotiate
	# with. `M79.0`: every service in this stack ran with
	# PGPROX_MOCK_TLS=disabled, so the proxy's default upstream mode had never
	# met a Postgres and could not have connected to one if it had.
	ssl = on
	ssl_cert_file = 'server.crt'
	ssl_key_file = 'server.key'
CONF
