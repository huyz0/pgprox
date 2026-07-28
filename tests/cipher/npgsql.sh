#!/usr/bin/env bash
# Npgsql over TLS against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$CIPHER_WORK/npgsql"
mkdir -p "$WORK"
cat > "$WORK/npgsql-cipher.csproj" <<'XML'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <RootNamespace>NpgsqlCipher</RootNamespace>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Npgsql" Version="9.*" />
  </ItemGroup>
</Project>
XML

cat > "$WORK/Program.cs" <<'CS'
using Npgsql;

// Trust Server Certificate because the stack's certificate is self-signed and
// made at start. The property under test is which suite the two sides agree
// on, not whether a name matches.
var connString =
    $"Host={Environment.GetEnvironmentVariable("PGPROX_HOST")};"
    + $"Port={Environment.GetEnvironmentVariable("PGPROX_PORT")};"
    + $"Username={Environment.GetEnvironmentVariable("PGPROX_USER")};"
    + $"Password={Environment.GetEnvironmentVariable("PGPROX_TOKEN")};"
    + $"Database={Environment.GetEnvironmentVariable("PGPROX_DB")};"
    + "SSL Mode=Require;Trust Server Certificate=true;"
    + "Max Auto Prepare=0;Server Compatibility Mode=NoTypeLoading";

await using var conn = new NpgsqlConnection(connString);
await conn.OpenAsync();

await using (var cmd = new NpgsqlCommand("SELECT 1", conn))
{
    var value = await cmd.ExecuteScalarAsync();
    if (Convert.ToInt32(value) != 1)
    {
        Console.Error.WriteLine("npgsql: query did not return 1");
        Environment.Exit(1);
    }
}

Console.WriteLine("npgsql: connected");
CS

cd "$WORK"
dotnet run --project npgsql-cipher.csproj --verbosity quiet 2>&1 | tail -3
