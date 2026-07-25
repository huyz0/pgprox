#!/usr/bin/env bash
# Npgsql against the conformance harness.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_harness.sh"

start_harness

WORK="$CONFORMANCE_ROOT/target/npgsql-check"
mkdir -p "$WORK"
cat > "$WORK/npgsql-check.csproj" <<'XML'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <RootNamespace>NpgsqlCheck</RootNamespace>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Npgsql" Version="9.*" />
  </ItemGroup>
</Project>
XML

cat > "$WORK/Program.cs" <<'CS'
using Npgsql;

var port = Environment.GetEnvironmentVariable("PGPROX_HARNESS_PORT");
var connString =
    $"Host=127.0.0.1;Port={port};Username=postgres;Database=conformance;SSL Mode=Disable;"
    + "Max Auto Prepare=0;Server Compatibility Mode=NoTypeLoading";

await using var conn = new NpgsqlConnection(connString);
await conn.OpenAsync();

await using (var cmd = new NpgsqlCommand("SELECT 1", conn))
{
    var value = await cmd.ExecuteScalarAsync();
    if (Convert.ToInt32(value) != 1)
    {
        Console.Error.WriteLine($"expected 1, got {value}");
        return 1;
    }
}

// Again on the same connection, so the sequence must have closed cleanly.
await using (var cmd = new NpgsqlCommand("SELECT 1", conn))
{
    _ = await cmd.ExecuteScalarAsync();
}

Console.WriteLine("npgsql: ok");
return 0;
CS

cd "$WORK"
dotnet run --project npgsql-check.csproj --verbosity quiet
