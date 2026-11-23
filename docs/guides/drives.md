# Drives and files

{.lead}
A drive holds files in a workspace, like a shared folder.

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create a drive

{rust:method}`create <DrivesClient::create>` takes the drive's ID, the name Nominal shows as its title. It can use letters, digits, `.`, `_` and `-`, and must be unique in the workspace.

```rust
let drive = client.drives().create("test-reports").await?;
```

## Fetch a drive

If you know a drive's RID or ID, you can fetch it directly without searching.

```rust
let drive = client.drives().get("<ri.file-store.instance.drive.identifier>").await?;

// Or by its ID, a name that's unique in its workspace
let drive = client.drives().get_by_id("test-reports").await?;
```

## Find drives

{rust:method}`list <DrivesClient::list>` returns the workspace's drives.

```rust
for drive in client.drives().list(false).await? { // false leaves out archived drives
    println!("{}: {}", drive.id(), drive.rid());
}
```

## Put a file

Put a file in a drive through the REST API. Follow the [Put a file in a drive](../examples/drive-put.md) example to upload it, then place it at a path in the drive.

## List a folder

{rust:method}`files <NominalClient::files>` returns an object for accessing one drive's files.

```rust
use nominal::core::FileEntry;

let drive_files = client.files(drive.rid());
for entry in drive_files.list("reports", false).await? { // false leaves out removed files
    match entry {
        FileEntry::File(file) => println!("{} ({} bytes)", file.path(), file.size_bytes()),
        FileEntry::Directory(directory) => println!("{}/", directory.path()),
    }
}
```

:::{note}
`""` is the drive's root. Paths have no leading prefix, such as `/` or `C:\`.
:::

## Download a file

{rust:method}`download <DriveFilesClient::download>` returns a reader of a file's contents.

```rust
// The reader streams from a link that expires, so copy from it right away
let mut reader = drive_files.download("reports/run-12.pdf").await?;
let mut local_file = tokio::fs::File::create("run-12.pdf").await?;
tokio::io::copy(&mut reader, &mut local_file).await?;
```

## Move and remove files

To move or remove a file, pass the RID of its current revision. The call fails if another change has replaced that revision since.

```rust
let file = drive_files.get("reports/run-12.pdf").await?;
let revision = file.current_revision_rid().ok_or("no current revision")?;

// Move the file. This fails if a file is already at the new path.
let moved = drive_files.move_file(revision, "archive/run-12.pdf").await?;

// Remove it. list shows it again only with removed files included.
let revision = moved.current_revision_rid().ok_or("no current revision")?;
drive_files.remove(revision).await?;
```

## Restore an earlier revision

{rust:method}`list_revisions <DriveFilesClient::list_revisions>` lists a file's revisions, oldest first. {rust:method}`restore <DriveFilesClient::restore>` puts an earlier revision back. While the file is in the drive, the revision replaces its current one. Once the file is removed, restore a revision at any path where no file is yet.

```rust
use nominal::core::FileOperationDestination;

let file = drive_files.get("reports/run-12.pdf").await?;
let file_rid = file.managed_file_rid().ok_or("no file RID")?;
let revisions = drive_files.list_revisions(file_rid).await?;
let first = &revisions[0];

// Put the first revision back in place of the file's current one
let current = file.current_revision_rid().ok_or("no current revision")?;
drive_files.restore(first.file_revision_rid(), FileOperationDestination::file_revision_rid(current)).await?;

// For a removed file, find it with list(…, true), then restore one of its revisions at a path where no file is yet
drive_files.restore("<ri.file-store.instance.drive-file-revision.identifier>", "reports/run-12.pdf").await?;
```

To access your drives from a terminal, see [nomctl fs](../ref/cli/fs.md).
