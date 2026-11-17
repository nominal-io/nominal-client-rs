# Drives and files

{.lead}
A drive holds files in a workspace, like a shared folder. Your program can list a drive's files and download them.

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create a drive

{rust:method}`create <DrivesClient::create>` creates a drive that stores its files in Nominal:

```rust
let drive = client.drives().create("test-reports").await?;
```

## Fetch a drive

If the RID of a drive is known, you can fetch that drive directly without needing to search.

```rust
let drive = client.drives().get("<ri.file-store.instance.drive.identifier>").await?;

// Or by its ID, a name that's unique in its workspace
let drive = client.drives().get_by_id("test-reports").await?;
```

## Find drives

{rust:method}`list <DrivesClient::list>` returns the workspace's drives. {rust:method}`source <Drive::source>` says whether a drive's files are stored in Nominal, S3 or Google Drive.

```rust
for drive in client.drives().list(false).await? {
    // true includes archived drives
    println!("{}: {}", drive.id(), drive.source());
}
```

## List a folder

{rust:method}`files <NominalClient::files>` returns a client for one drive's files. Paths have no leading `/`, and `""` is the drive's root.

```rust
use nominal::core::FileEntry;

let drive_files = client.files(drive.rid());
for entry in drive_files.list("reports", false).await? {
    // true includes removed files
    match entry {
        FileEntry::File(file) => println!("{} ({} bytes)", file.path(), file.size_bytes()),
        FileEntry::Directory(directory) => println!("{}/", directory.path()),
    }
}
```

## Download a file

{rust:method}`download <DriveFilesClient::download>` works on drives that store their files in Nominal.

```rust
// The reader streams from a link that expires, so copy from it right away
let mut reader = drive_files.download("reports/run-12.pdf").await?;
let mut local_file = tokio::fs::File::create("run-12.pdf").await?;
tokio::io::copy(&mut reader, &mut local_file).await?;
```

To work with drives from a terminal, see [nomctl fs](../ref/cli/fs.md).
