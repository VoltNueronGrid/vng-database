# VoltNueronGrid Tools for Visual Studio

## Overview

VSIX extension for Visual Studio 2022+ that provides:
- **Connection wizard** — host, port, admin API key, database name (stored in VS options).
- **Schema browser** — tree view of databases → tables → columns via `GET /api/v1/catalog/list`.
- **SQL editor** — write and execute SQL via `POST /api/v1/sql/execute`.
- **Result grid** — tabular display of `oltp_rows` from the API response.
- **Query plan inspector** — shows `EXPLAIN SELECT` output from the planner.

## Build

Requires:
- Visual Studio 2022 with "Visual Studio extension development" workload
- .NET Framework 4.7.2

```powershell
# Restore and build the VSIX package
dotnet restore VoltNueronGrid.VS.Extension.csproj
msbuild VoltNueronGrid.VS.Extension.csproj /t:Build /p:Configuration=Release
```

The output `.vsix` file appears in `bin/Release/`.

## Install

Double-click the `.vsix` file, or use **Extensions → Manage Extensions → Install from VSIX**.

## Development

Open `VoltNueronGrid.VS.Extension.csproj` in Visual Studio 2022 with the extension development workload.
Press F5 to launch the Experimental Instance.

## Files

| File | Purpose |
|---|---|
| `source.extension.vsixmanifest` | VSIX manifest (id, name, install targets) |
| `VngPackage.cs` | VS AsyncPackage entry point |
| `VngConnectionOptions.cs` | Tools > Options settings page |
| `VoltNueronGridClient/VngApiClient.cs` | Typed HTTP client for the VNG REST API |
| `ToolWindows/VngToolWindow.cs` | Tool window host |
| `ToolWindows/VngToolWindowControl.cs` | WPF UI (tabs: Connection, Schema, SQL Editor, Results) |
| `Commands/ExecuteSqlCommand.cs` | Menu command: execute SQL |
| `Commands/OpenConnectionCommand.cs` | Menu command: open connection tool window |
