#!/usr/bin/env bash
# The PostgreSQL JDBC driver over TLS against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$CIPHER_WORK/jdbc"
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

cat > "$WORK/JdbcCipher.java" <<'JAVA'
import java.sql.*;

public class JdbcCipher {
    public static void main(String[] args) throws Exception {
        String url = "jdbc:postgresql://" + System.getenv("PGPROX_HOST")
            + ":" + System.getenv("PGPROX_PORT")
            + "/" + System.getenv("PGPROX_DB")
            // NonValidatingFactory because the stack's certificate is
            // self-signed and made at start. Without it this probe would be
            // testing trust rather than cipher negotiation.
            + "?ssl=true&sslmode=require"
            + "&sslfactory=org.postgresql.ssl.NonValidatingFactory";

        try (Connection conn = DriverManager.getConnection(
                url, System.getenv("PGPROX_USER"), System.getenv("PGPROX_TOKEN"));
             Statement st = conn.createStatement();
             ResultSet rs = st.executeQuery("SELECT 1")) {
            if (!rs.next() || rs.getInt(1) != 1) {
                System.err.println("jdbc: query did not return 1");
                System.exit(1);
            }
        }
        System.out.println("jdbc: connected");
    }
}
JAVA

cd "$WORK"
java -cp "$JAR" JdbcCipher.java
