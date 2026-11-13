# Drives and files

{.lead}
A drive is a named container of files in a workspace, like a bucket or a shared folder. `client.drives()` creates and finds drives, and `client.files(drive_rid)` puts, lists and downloads the files in one.

## Find or create a drive

Each drive has an ID, a name that's unique in its workspace, and a RID:

```rust
let drive = client.drives().create("test-reports").await?;

let drive = client.drives().get_by_id("test-reports").await?;
let drive = client.drives().get("<ri.filestore.instance.drive.identifier>").await?;
```

`list` returns the workspace's drives. Pass `true` to include archived ones:

```rust
for drive in client.drives().list(false).await? {
    println!("{}: {:?}, {:?}", drive.id(), drive.source(), drive.content_mutability());
}
```

A drive's files are stored in Nominal, or come from S3 or Google Drive. `source()` says which, and `content_mutability()` says whether you can change them through Nominal. Changing files works only in a writable drive.

## Put a file

File operations go through a client for one drive. Paths are relative to the drive's root, with no leading `/`:

```rust
use nominal::core::UploadOptions;

let files = client.files(drive.rid());
let file = files.put("run-12-report.pdf", "reports/run-12.pdf", UploadOptions::new()).await?;
println!("{} ({} bytes)", file.path(), file.size_bytes());
```

`put` fails if a file already exists at the path. `UploadOptions` sets the upload's part size, concurrency and retries, and can report progress, as in [Ingest jobs](ingest-jobs.md#upload-options-and-progress).

## List a folder

`list` returns the files and folders directly under a path. An empty path lists the drive's root:

```rust
use nominal::core::FileEntry;

for entry in files.list("reports", false).await? {
    match entry {
        FileEntry::File(file) => println!("{} ({} bytes)", file.path(), file.size_bytes()),
        FileEntry::Directory(dir) => println!("{}/", dir.path()),
    }
}
```

Pass `true` to include removed files. To look up one file, use `get`:

```rust
let file = files.get("reports/run-12.pdf").await?;
```

## Download a file

`download` returns a reader over the file's current content. Read it promptly: it streams from a link that expires after a short time.

```rust
let mut reader = files.download("reports/run-12.pdf").await?;
let mut local = tokio::fs::File::create("run-12.pdf").await?;
tokio::io::copy(&mut reader, &mut local).await?;
```

## Move and remove files

Moving or removing a file names its current revision, and fails if that revision is no longer the file's current one:

```rust
let file = files.get("reports/run-12.pdf").await?;
let revision = file.current_revision_rid().expect("a file in a drive Nominal stores");

let moved = files.move_file(revision, "archive/run-12.pdf").await?;
```

A move fails if a file exists at the destination. `remove` removes a file, which then shows in `list` only with removed files included:

```rust
let revision = moved.current_revision_rid().expect("a file in a drive Nominal stores");
files.remove(revision).await?;
```

Files from S3 or Google Drive have no revision RID, so they can't be moved or removed this way.

## Restore a past revision

`list_revisions` lists a file's revisions, oldest first. `restore` puts a past revision back, either at a new path, which must not exist yet, or in place of the file a revision RID points to:

```rust
use nominal::core::FileOperationDestination;

let file = files.get("reports/run-12.pdf").await?;
let file_rid = file.managed_file_rid().expect("a file in a drive Nominal stores");
let revisions = files.list_revisions(file_rid).await?;
let first = &revisions[0];

// At a new path
files.restore(first.file_revision_rid(), "reports/run-12-original.pdf").await?;

// In place of the file whose current revision this is
let current = file.current_revision_rid().expect("a file in a drive Nominal stores");
files.restore(first.file_revision_rid(), FileOperationDestination::file_revision_rid(current)).await?;
```

## Rename and archive drives

```rust
let drive = client.drives().rename(drive.rid(), "bench-reports").await?;
client.drives().archive(drive.rid()).await?;
client.drives().unarchive(drive.rid()).await?;
```

Archiving hides a drive in Nominal without deleting it.

[nomctl's `fs` commands](../ref/cli/fs.md) do the same from a terminal, with paths written as `DRIVE:/PATH`.
