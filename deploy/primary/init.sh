#!/usr/bin/env bash
# The primary, prepared for both replication and the tenant the e2e run uses.
#
# Runs once, from the postgres image's own init hook, so it is guaranteed to
# happen before the server accepts a connection from anything else.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" <<-SQL
	CREATE ROLE replicator WITH REPLICATION LOGIN PASSWORD 'replicator';
	CREATE ROLE acme_app WITH LOGIN PASSWORD 'acme-password';
	CREATE DATABASE tenant_acme OWNER acme_app;
SQL

# Replication and the tenant both connect from inside the compose network, so
# the rules are scoped to it rather than to the world. This is a test stack: a
# real deployment uses certificates and the tenant's own password policy.
cat >> "$PGDATA/pg_hba.conf" <<-HBA
	host replication replicator all md5
	host all         all        all md5
HBA

cat >> "$PGDATA/postgresql.conf" <<-CONF
	wal_level = replica
	max_wal_senders = 10
	max_replication_slots = 10
	hot_standby = on
	# Small, so a replica's lag is visible to the e2e run rather than hidden
	# by the primary batching its WAL.
	wal_writer_delay = 10ms
	# Enough WAL kept that a replica cloning while the primary is being written
	# to does not lose the segment it started from. Without it pg_basebackup
	# fails with "requested WAL segment has already been removed", which reads
	# like a network fault and is not one.
	wal_keep_size = 256MB
CONF
