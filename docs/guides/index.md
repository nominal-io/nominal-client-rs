# Guides

These guides go further than the [quickstart](../index.md), along the common path for each task. Each example assumes a `client` created as in [Connect to Nominal](connect.md), inside an async function that returns a `Result`.

## Get started

- [API Keys Setup](api-keys-setup.md): copy your API key, base URL and workspace RID, and save them in a profile.
- [Connect to Nominal](connect.md): create a client from a profile, or from an API key and URL.
- [Cheat sheet](cheatsheet.md): the common calls, one line each.

## Organize data

- [Assets](assets.md): the systems you test, and the data scopes that hold their datasets.
- [Runs](runs.md): the time span of each test, on one or more assets.
- [Datasets](datasets.md): create, find and update datasets.
- [Channels](channels.md): find channels, and set their units and descriptions.

## Get data in

- [Upload files](upload.md): CSV, Parquet, MCAP, journal JSON, Avro streams, ArduPilot DataFlash and video.
- [Ingest jobs](ingest-jobs.md): wait for an upload's ingest, check its status, and follow upload progress.
- [Stream data](stream.md): send points to a dataset as your program produces them.
- [Drives and files](drives.md): create drives, and list and download their files.

## Review data

- [Workbooks](workbooks.md): create workbooks from templates for assets and runs.
- [Checklists](checklists.md): run checklists on streamed data, and read their live status.
- [Get data out with SQL](sql.md): query your data, and export large results to a file.

## Beyond the crate

- [Call the REST API directly](rest-api.md): events, creating checklists, running a checklist on a run, and runs over tagged data.

For complete programs, see [Examples](../examples/index.md).
