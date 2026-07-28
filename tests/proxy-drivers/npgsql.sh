#!/usr/bin/env bash
# Npgsql against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$PROBE_WORK/npgsql"
mkdir -p "$WORK"
cat > "$WORK/npgsql-proxy.csproj" <<'XML'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <RootNamespace>NpgsqlProxy</RootNamespace>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Npgsql" Version="9.*" />
  </ItemGroup>
</Project>
XML

cat > "$WORK/Program.cs" <<'CS'
using Npgsql;

static void Die(string what)
{
    Console.Error.WriteLine($"npgsql: {what}");
    Environment.Exit(1);
}

// Trust Server Certificate because the stack's certificate is self-signed and
// made at start. What is under test is the protocol behind the handshake.
var connString =
    $"Host={Environment.GetEnvironmentVariable("PGPROX_HOST")};"
    + $"Port={Environment.GetEnvironmentVariable("PGPROX_PORT")};"
    + $"Username={Environment.GetEnvironmentVariable("PGPROX_USER")};"
    + $"Password={Environment.GetEnvironmentVariable("PGPROX_TOKEN")};"
    + $"Database={Environment.GetEnvironmentVariable("PGPROX_DB")};"
    + "SSL Mode=Require;Trust Server Certificate=true;"
    + "Server Compatibility Mode=NoTypeLoading";

await using var conn = new NpgsqlConnection(connString);
await conn.OpenAsync();

await using (var cmd = new NpgsqlCommand("SELECT 1", conn))
{
    if (Convert.ToInt32(await cmd.ExecuteScalarAsync()) != 1) Die("simple query did not return 1");
}

// A bound parameter, which is the extended protocol.
await using (var cmd = new NpgsqlCommand("SELECT $1::int + 1", conn))
{
    cmd.Parameters.Add(new NpgsqlParameter { Value = 41 });
    if (Convert.ToInt32(await cmd.ExecuteScalarAsync()) != 42) Die("a bound parameter came back wrong");
}

// PGPROX_DEPTH_PREPARED_REUSE. Prepare() makes it a named server-side
// statement, and every execute after that sends Bind alone.
await using (var cmd = new NpgsqlCommand("SELECT $1::int", conn))
{
    cmd.Parameters.Add(new NpgsqlParameter { Value = 7 });
    await cmd.PrepareAsync();
    for (var i = 0; i < 5; i++)
    {
        if (Convert.ToInt32(await cmd.ExecuteScalarAsync()) != 7) Die("prepared reuse came back wrong");
    }
}

// PGPROX_DEPTH_LARGE_RESULT.
await using (var cmd = new NpgsqlCommand("SELECT generate_series(1, 5000)", conn))
await using (var reader = await cmd.ExecuteReaderAsync())
{
    var count = 0;
    while (await reader.ReadAsync()) count++;
    if (count != 5000) Die($"large result gave {count} rows");
}

// A transaction, which is what the pool releases on.
await using (var tx = await conn.BeginTransactionAsync())
{
    await using var cmd = new NpgsqlCommand("SELECT 2", conn, tx);
    if (Convert.ToInt32(await cmd.ExecuteScalarAsync()) != 2) Die("statement in a transaction failed");
    await tx.CommitAsync();
}

// An error, and a statement after it.
try
{
    await using var bad = new NpgsqlCommand("SELECT no_such_column_xyz", conn);
    await bad.ExecuteScalarAsync();
    Die("a bad column succeeded");
}
catch (PostgresException)
{
    // The point.
}

await using (var cmd = new NpgsqlCommand("SELECT 3", conn))
{
    if (Convert.ToInt32(await cmd.ExecuteScalarAsync()) != 3) Die("statement after an error failed");
}

Console.WriteLine("npgsql: ok");
CS

cd "$WORK"
dotnet run --project npgsql-proxy.csproj --verbosity quiet 2>&1 | tail -3
