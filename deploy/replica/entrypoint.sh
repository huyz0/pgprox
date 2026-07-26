#!/usr/bin/env bash
# A streaming replica, built from the primary at container start.
#
# pg_basebackup rather than a volume copy, because a replica has to have been
# cloned from the primary it follows: two independently initialised clusters
# have different system identifiers and replication refuses them.
set -euo pipefail

PRIMARY="${PRIMARY_HOST:-primary}"

if [ ! -s "$PGDATA/PG_VERSION" ]; then
  echo "waiting for $PRIMARY before cloning"
  until pg_isready --host "$PRIMARY" --username postgres --quiet; do
    sleep 1
  done

  # Retried, because the primary finishes its own initialisation after it
  # starts answering pg_isready: a clone that begins in that window can lose
  # the WAL segment it started from, and the fix is to start again rather than
  # to bring up a replica that will never catch up.
  attempt=1
  until [ "$attempt" -gt 5 ]; do
    rm -rf "${PGDATA:?}"/*
    if PGPASSWORD=replicator pg_basebackup \
        --host "$PRIMARY" --username replicator \
        --pgdata "$PGDATA" --wal-method=stream --write-recovery-conf --progress; then
      break
    fi
    echo "clone attempt $attempt failed, retrying"
    attempt=$((attempt + 1))
    sleep 3
  done

  if [ ! -s "$PGDATA/PG_VERSION" ]; then
    echo "could not clone $PRIMARY after $((attempt - 1)) attempts"
    exit 1
  fi
  chmod 0700 "$PGDATA"
fi

exec postgres
