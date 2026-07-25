#!/usr/bin/env bash
# The PostgreSQL JDBC driver against the conformance harness.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_harness.sh"

start_harness

WORK="$CONFORMANCE_ROOT/target/jdbc-check"
mkdir -p "$WORK"

# Fetch the driver once, into the work directory rather than the user's cache
# root, so a clean checkout behaves the same as a warm one.
JAR="$WORK/postgresql.jar"
if [[ ! -f "$JAR" ]]; then
  mvn -q dependency:get -Dartifact=org.postgresql:postgresql:42.7.4 >/dev/null 2>&1
  mvn -q dependency:copy \
    -Dartifact=org.postgresql:postgresql:42.7.4 \
    -DoutputDirectory="$WORK" \
    -Dmdep.stripVersion=true >/dev/null 2>&1
  mv "$WORK/postgresql.jar" "$JAR" 2>/dev/null || true
fi
[[ -f "$JAR" ]] || { echo "could not fetch the JDBC driver" >&2; exit 1; }

cat > "$WORK/JdbcCheck.java" <<'JAVA'
import java.sql.*;

public class JdbcCheck {
    public static void main(String[] args) throws Exception {
        String port = System.getenv("PGPROX_HARNESS_PORT");
        String url = "jdbc:postgresql://127.0.0.1:" + port + "/conformance?sslmode=disable";

        try (Connection conn = DriverManager.getConnection(url, "postgres", "")) {
            // Statement uses the simple query protocol.
            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT 1")) {
                if (!rs.next() || rs.getInt(1) != 1) {
                    System.err.println("simple query did not return 1");
                    System.exit(1);
                }
            }

            // PreparedStatement uses the extended one, which is the path that
            // matters for prepared statement mapping.
            try (PreparedStatement ps = conn.prepareStatement("SELECT 1")) {
                for (int i = 0; i < 2; i++) {
                    try (ResultSet rs = ps.executeQuery()) {
                        if (!rs.next() || rs.getInt(1) != 1) {
                            System.err.println("prepared query did not return 1");
                            System.exit(1);
                        }
                    }
                }
            }
        }
        System.out.println("jdbc: ok");
    }
}
JAVA

java -cp "$JAR" "$WORK/JdbcCheck.java"
