# Drives and files

{.lead}
A drive is a named container of files in a workspace, like a bucket or a shared folder. `client.drives()` creates and finds drives, and `client.files(drive_rid)` lists and downloads the files in one.

## Find or create a drive

Each drive has an ID, a name that's unique in its workspace, and a RID:

```rust
let drive = client.drives().create("test-reports").await?;

let drive = client.drives().get_by_id("test-reports").await?;
let drive = client.drives().get("<ri.file-store.instance.drive.identifier>").await?;
```

`list` returns the workspace's drives. Pass `true` to include archived ones:

```rust
for drive in client.drives().list(false).await? {
    println!("{}: {:?}, {:?}", drive.id(), drive.source(), drive.content_mutability());
}
```

A drive's files are stored in Nominal, or come from S3 or Google Drive. `source()` says which, and `content_mutability()` says whether you can change them through Nominal.

## List a folder

File operations go through a client for one drive. Paths are relative to the drive's root, with no leading `/`. `list` returns the files and folders directly under a path, and an empty path lists the drive's root:

```rust
use nominal::core::FileEntry;

let files = client.files(drive.rid());
for entry in files.list("reports", false).await? {
    match entry {
        FileEntry::File(file) => println!("{} ({} bytes)", file.path(), file.size_bytes()),
        FileEntry::Directory(dir) => println!("{}/", dir.path()),
    }
}
```

Pass `true` to include files that have been removed. To look up one file, use `get`:

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

## Rename and archive drives

```rust
let drive = client.drives().rename(drive.rid(), "bench-reports").await?;
client.drives().archive(drive.rid()).await?;
client.drives().unarchive(drive.rid()).await?;
```

Archiving hides a drive in Nominal without deleting it.

[nomctl's `fs` commands](../ref/cli/fs.md) do the same from a terminal, with paths written as `DRIVE:/PATH`.
