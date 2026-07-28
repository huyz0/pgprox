#!/usr/bin/env bash
# The PostgreSQL JDBC driver against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$PROBE_WORK/jdbc"
mkdir -p "$WORK"

JAR="$WORK/postgresql.jar"
if [[ ! -f "$JAR" ]]; then
  mvn -q dependency:get -Dartifact=org.postgresql:postgresql:42.7.4 >/dev/null 2>&1
  mvn -q dependency:copy \
    -Dartifact=org.postgresql:postgresql:42.7.4 \
    -DoutputDirectory="$WORK" \
    -Dmdep.stripVersion=true >/dev/null 2>&1
fi
[[ -f "$JAR" ]] || { echo "could not fetch the JDBC driver" >&2; exit 1; }

cat > "$WORK/JdbcProxy.java" <<'JAVA'
import java.sql.*;

public class JdbcProxy {
    static void die(String what) {
        System.err.println("jdbc: " + what);
        System.exit(1);
    }

    public static void main(String[] args) throws Exception {
        String url = "jdbc:postgresql://" + System.getenv("PGPROX_HOST")
            + ":" + System.getenv("PGPROX_PORT")
            + "/" + System.getenv("PGPROX_DB")
            // NonValidatingFactory because the stack's certificate is
            // self-signed and made at start. Without it this would be a trust
            // test rather than a protocol one.
            + "?ssl=true&sslmode=require"
            + "&sslfactory=org.postgresql.ssl.NonValidatingFactory";

        try (Connection conn = DriverManager.getConnection(
                url, System.getenv("PGPROX_USER"), System.getenv("PGPROX_TOKEN"))) {

            // Statement is the simple query protocol.
            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT 1")) {
                if (!rs.next() || rs.getInt(1) != 1) die("simple query did not return 1");
            }

            // PreparedStatement is the extended one. The JDBC driver switches
            // to a named server-side statement after prepareThreshold uses,
            // which is the behaviour statement mapping exists for.
            try (PreparedStatement ps = conn.prepareStatement("SELECT ?::int + 1")) {
                // PGPROX_DEPTH_PREPARED_REUSE.
                for (int i = 0; i < 6; i++) {
                    ps.setInt(1, 41);
                    try (ResultSet rs = ps.executeQuery()) {
                        if (!rs.next() || rs.getInt(1) != 42) die("prepared reuse gave the wrong answer");
                    }
                }
            }

            // PGPROX_DEPTH_LARGE_RESULT.
            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT generate_series(1, 5000)")) {
                int count = 0;
                while (rs.next()) count++;
                if (count != 5000) die("large result gave " + count + " rows");
            }

            // A transaction, which is what the pool releases on.
            conn.setAutoCommit(false);
            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT 2")) {
                if (!rs.next() || rs.getInt(1) != 2) die("statement in a transaction failed");
            }
            conn.commit();
            conn.setAutoCommit(true);

            // An error, and a statement after it.
            try (Statement st = conn.createStatement()) {
                st.executeQuery("SELECT no_such_column_xyz");
                die("a bad column succeeded");
            } catch (SQLException expected) {
                // The point.
            }
            try (Statement st = conn.createStatement();
                 ResultSet rs = st.executeQuery("SELECT 3")) {
                if (!rs.next() || rs.getInt(1) != 3) die("statement after an error failed");
            }
        }
        System.out.println("jdbc: ok");
    }
}
JAVA

cd "$WORK"
java -cp "$JAR" JdbcProxy.java
