# Extract Apple Audiobooks

Export audiobooks from Apple Books to [Audiobookshelf](https://www.audiobookshelf.org/)-compatible format.

Apple Books stores audiobooks in a cryptic folder structure using SHA1 hashes. This tool reads the `Books.plist` metadata file and exports your audiobooks into a clean `Author/Title/` directory structure that Audiobookshelf can automatically scan and import.

## Features

- Exports all audiobooks from Apple Books library
- Preserves metadata (author, title, narrator)
- Creates Audiobookshelf-compatible directory structure
- **Symlink support** - save disk space by linking instead of copying
- **Dry-run with diff** - preview what will be copied before running
- ⏭ Skips files that already exist at destination
- Works with external drives and custom library locations

## Installation

```bash
# Run directly with cargo
cargo run -- --help

# Or build a release binary
cargo build --release
./target/release/extract_apple_books --help
```

## Usage

### Basic Usage

```bash
# Export to a destination folder (uses default Apple Books location)
cargo run -- --dest /path/to/audiobooks

# Specify a custom source location (e.g., external drive backup)
cargo run -- \
  --source /Volumes/backup/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books \
  --dest /path/to/audiobooks
```

### Dry Run (Preview Changes)

Use `--dry-run` to see what would be copied without making any changes:

```bash
cargo run -- --dest /path/to/audiobooks --dry-run
```

**Sample output:**

```
Reading audiobook library from: "/Users/charlie/Library/.../Books.plist"
Found 128 audiobooks

=== DRY RUN - No files will be copied ===

╔══════════════════════════════════════════════════════════════════╗
║                        DIFF SUMMARY                               ║
╚══════════════════════════════════════════════════════════════════╝

┌─────────────────────────────────────────────────────────────────┐
│ + TO ADD (891 files in 122 books)
└─────────────────────────────────────────────────────────────────┘
  + Adrian Tchaikovsky - Children of Ruin (Unabridged) (1 files)
  + Adrian Tchaikovsky - Children of Time (Unabridged) (1 files)
  + Andy Weir - Project Hail Mary (1 files)
  + Barack Obama - A Promised Land (1 files)
  + Bill Bryson - A Short History of Nearly Everything (Unabridged) (1 files)
  ... and 117 more books

┌─────────────────────────────────────────────────────────────────┐
│ = ALREADY EXISTS (65 files in 6 books)
└─────────────────────────────────────────────────────────────────┘
  = John Scalzi - The Dispatcher (Unabridged)
  = Ryan Holiday - Stillness Is the Key (Unabridged)
  = Salman Rushdie - The Satanic Verses
  ... and 3 more books

┌─────────────────────────────────────────────────────────────────┐
│ TOTALS                                                          │
├─────────────────────────────────────────────────────────────────┤
│  + New files to copy:        891                               │
│  = Already exist (skip):      65                               │
│  ! Source missing:             0                               │
└─────────────────────────────────────────────────────────────────┘
```

### Using Symlinks to Save Space

If your Apple Books library is on the same filesystem as your destination, you can use symlinks instead of copying files. This saves significant disk space since audiobooks can be several gigabytes each.

```bash
cargo run -- --dest /path/to/audiobooks --symlink
```

**Benefits of symlinks:**

- ✅ Instant "copying" - no waiting for large files to transfer
- ✅ Zero additional disk space used
- ✅ Changes to source files are automatically reflected
- ✅ Perfect for local Audiobookshelf setups

**Caveats:**

- ⚠️ Source and destination must be on the same filesystem
- ⚠️ If you delete the original Apple Books library, the symlinks will break
- ⚠️ May not work if Audiobookshelf runs in a container without access to the source path

## Output Structure

The tool creates an Audiobookshelf-compatible directory structure:

```
/path/to/audiobooks/
├── Adrian Tchaikovsky/
│   ├── Children of Ruin (Unabridged)/
│   │   └── Children of Ruin.m4b
│   └── Children of Time (Unabridged)/
│       └── Children of Time.m4b
├── Andy Weir/
│   └── Project Hail Mary/
│       └── Project Hail Mary.m4b
├── Brandon Sanderson/
│   └── Warbreaker/
│       ├── 01 WARBREAKER2P01.mp3
│       ├── 02 WARBREAKER2P02.mp3
│       └── ...
└── ...
```

If narrator information is available, it's included in the folder name:

```
Author Name/
└── Book Title {Narrator Name}/
    └── audiobook.m4b
```

## Importing into Audiobookshelf

Simply copy the directory structure to your ABS server, or export directly there, and add/scan a library.

## Command Line Options

| Option                | Description                                                                                                                   |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `-s, --source <PATH>` | Source path to Apple Books directory. Defaults to `~/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books` |
| `-d, --dest <PATH>`   | **Required.** Destination path for exported audiobooks                                                                        |
| `--dry-run`           | Show what would be copied without actually copying. Displays a diff summary.                                                  |
| `--symlink`           | Create symlinks instead of copying files (saves disk space)                                                                   |

## Default Apple Books Location

On macOS, Apple Books stores audiobooks at:

```
~/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books/
```

The metadata is in `Books.plist` and audiobook files are in:

```
~/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books/Audiobooks/sha1-xxxx/
```

## Running Tests

```bash
cargo test
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Troubleshooting

### "Permission denied" errors

Make sure you have read access to the Apple Books library. On macOS, you may need to grant Terminal (or your shell) Full Disk Access in **System Preferences → Security & Privacy → Privacy → Full Disk Access**.

### Symlinks not working in Audiobookshelf

If using Docker, ensure both the symlink destination AND the original source path are mounted in the container. Symlinks must be resolvable from within the container.

### Missing audiobooks

Some audiobooks may be stored in iCloud. Make sure they're downloaded locally in Apple Books before exporting.
