use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use plist::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExportError {
    #[error("Books.plist not found at {0}")]
    PlistNotFound(PathBuf),
    #[error("Invalid plist structure: {0}")]
    InvalidPlistStructure(String),
    #[error("No audiobooks found in library")]
    NoAudiobooksFound,
}

/// Export audiobooks from Apple Books to Audiobookshelf-compatible format
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Source path to Apple Books audiobooks directory
    /// Defaults to ~/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books
    #[arg(short, long)]
    source: Option<PathBuf>,

    /// Destination path for exported audiobooks
    #[arg(short, long)]
    dest: PathBuf,

    /// Dry run - show what would be copied without actually copying
    #[arg(long, default_value = "false")]
    dry_run: bool,

    /// Use symlinks instead of copying files
    #[arg(long, default_value = "false")]
    symlink: bool,
}

#[derive(Debug, Clone)]
pub struct Audiobook {
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub folder_id: String,
    pub tracks: Vec<AudioTrack>,
}

#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub track_number: u32,
    pub disc_number: u32,
    pub title: String,
    pub path: PathBuf,
    pub filename: String,
}

/// Get the default Apple Books path for the current user
fn default_apple_books_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books")
}

/// Parse the Books.plist file and extract audiobook metadata
pub fn parse_books_plist(plist_path: &Path) -> Result<Vec<Audiobook>> {
    if !plist_path.exists() {
        return Err(ExportError::PlistNotFound(plist_path.to_path_buf()).into());
    }

    let plist_value: Value = plist::from_file(plist_path)
        .with_context(|| format!("Failed to parse plist at {:?}", plist_path))?;

    let dict = plist_value
        .as_dictionary()
        .ok_or_else(|| ExportError::InvalidPlistStructure("Root is not a dictionary".into()))?;

    let books_array = dict
        .get("Books")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ExportError::InvalidPlistStructure("Missing 'Books' array".into()))?;

    let mut audiobooks = Vec::new();

    for book_value in books_array {
        if let Some(audiobook) = parse_audiobook_entry(book_value)? {
            audiobooks.push(audiobook);
        }
    }

    if audiobooks.is_empty() {
        return Err(ExportError::NoAudiobooksFound.into());
    }

    Ok(audiobooks)
}

/// Parse a single audiobook entry from the plist
fn parse_audiobook_entry(value: &Value) -> Result<Option<Audiobook>> {
    let dict = match value.as_dictionary() {
        Some(d) => d,
        None => return Ok(None),
    };

    // Check if this is an audiobook
    let book_type = dict
        .get("BKBookType")
        .and_then(|v| v.as_string())
        .unwrap_or("");

    if book_type != "audiobook" {
        return Ok(None);
    }

    let folder_id = dict
        .get("BKGeneratedItemId")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();

    let author = dict
        .get("artistName")
        .and_then(|v| v.as_string())
        .unwrap_or("Unknown Author")
        .to_string();

    // Parse tracks to get title and other metadata
    let parts = dict.get("BKParts").and_then(|v| v.as_array());

    let mut tracks = Vec::new();
    let mut title = String::new();
    let mut narrator: Option<String> = None;

    if let Some(parts_array) = parts {
        for part_value in parts_array {
            if let Some(part_dict) = part_value.as_dictionary() {
                // Get title from first track if not set
                if title.is_empty() {
                    title = part_dict
                        .get("itemName")
                        .and_then(|v| v.as_string())
                        .unwrap_or("Unknown Title")
                        .to_string();
                }

                // Try to get narrator from composer field (common in audiobooks)
                if narrator.is_none() {
                    narrator = part_dict
                        .get("composer")
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());
                }

                let track_number = part_dict
                    .get("BKTrackNumber")
                    .and_then(|v| v.as_unsigned_integer())
                    .unwrap_or(0) as u32;

                let disc_number = part_dict
                    .get("BKDiscNumber")
                    .and_then(|v| v.as_unsigned_integer())
                    .unwrap_or(0) as u32;

                let track_title = part_dict
                    .get("BKTrackTitle")
                    .and_then(|v| v.as_string())
                    .unwrap_or("")
                    .to_string();

                let path_str = part_dict
                    .get("path")
                    .and_then(|v| v.as_string())
                    .unwrap_or("");

                if !path_str.is_empty() {
                    let path = PathBuf::from(path_str);
                    let filename = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();

                    tracks.push(AudioTrack {
                        track_number,
                        disc_number,
                        title: track_title,
                        path,
                        filename,
                    });
                }
            }
        }
    }

    // Sort tracks by disc number then track number
    tracks.sort_by(|a, b| {
        a.disc_number
            .cmp(&b.disc_number)
            .then(a.track_number.cmp(&b.track_number))
    });

    if title.is_empty() || tracks.is_empty() {
        return Ok(None);
    }

    Ok(Some(Audiobook {
        title,
        author,
        narrator,
        folder_id,
        tracks,
    }))
}

/// Sanitize a string for use as a filename/directory name
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Create Audiobookshelf-compatible folder name for an audiobook
/// Format: Author/Title {Narrator} or Author/Title
fn create_audiobookshelf_path(dest: &Path, audiobook: &Audiobook) -> PathBuf {
    let author_dir = sanitize_filename(&audiobook.author);

    let title_dir = if let Some(ref narrator) = audiobook.narrator {
        format!(
            "{} {{{}}}",
            sanitize_filename(&audiobook.title),
            sanitize_filename(narrator)
        )
    } else {
        sanitize_filename(&audiobook.title)
    };

    dest.join(author_dir).join(title_dir)
}

/// Remap the source path in a track to use the actual source base path
/// The plist contains paths like /Users/charlie/Library/... but we might be reading from /Volumes/charlie/Library/...
fn remap_track_path(track_path: &Path, source_base: &Path) -> PathBuf {
    // Extract the relative path after "Audiobooks/" (the sha1 folder and filename)
    let path_str = track_path.to_string_lossy();

    if let Some(idx) = path_str.find("Audiobooks/") {
        // Get just the part starting from "Audiobooks/"
        let relative = &path_str[idx..];
        source_base.join(relative)
    } else {
        // Fallback: try to find just the audiobook folder and filename
        let components: Vec<_> = track_path.components().collect();
        if components.len() >= 2 {
            // Get the sha1-xxx folder and filename
            let folder = components[components.len() - 2]
                .as_os_str()
                .to_string_lossy();
            let filename = components[components.len() - 1]
                .as_os_str()
                .to_string_lossy();

            if folder.starts_with("sha1-") {
                return source_base
                    .join("Audiobooks")
                    .join(folder.as_ref())
                    .join(filename.as_ref());
            }
        }
        track_path.to_path_buf()
    }
}

/// Export audiobooks to the destination directory
pub fn export_audiobooks(
    audiobooks: &[Audiobook],
    source_base: &Path,
    dest: &Path,
    dry_run: bool,
    use_symlink: bool,
) -> Result<ExportStats> {
    let mut stats = ExportStats::default();

    let pb = ProgressBar::new(audiobooks.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    for audiobook in audiobooks {
        pb.set_message(format!("{} - {}", audiobook.author, audiobook.title));

        let dest_dir = create_audiobookshelf_path(dest, audiobook);

        if !dry_run {
            fs::create_dir_all(&dest_dir)
                .with_context(|| format!("Failed to create directory {:?}", dest_dir))?;
        }

        for track in &audiobook.tracks {
            let source_path = remap_track_path(&track.path, source_base);
            let dest_path = dest_dir.join(&track.filename);

            if dry_run {
                println!(
                    "Would {} {:?} -> {:?}",
                    if use_symlink { "symlink" } else { "copy" },
                    source_path,
                    dest_path
                );
                stats.files_would_copy += 1;
            } else if !source_path.exists() {
                eprintln!("Warning: Source file not found: {:?}", source_path);
                stats.source_missing += 1;
            } else if dest_path.exists() {
                // Skip files that already exist
                stats.files_already_exist += 1;
            } else {
                if use_symlink {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(&source_path, &dest_path).with_context(
                            || format!("Failed to symlink {:?} -> {:?}", source_path, dest_path),
                        )?;
                    }
                    #[cfg(not(unix))]
                    {
                        fs::copy(&source_path, &dest_path).with_context(|| {
                            format!("Failed to copy {:?} -> {:?}", source_path, dest_path)
                        })?;
                    }
                } else {
                    fs::copy(&source_path, &dest_path).with_context(|| {
                        format!("Failed to copy {:?} -> {:?}", source_path, dest_path)
                    })?;
                }
                stats.files_copied += 1;
            }
        }

        stats.books_exported += 1;
        pb.inc(1);
    }

    pb.finish_with_message("Done!");

    Ok(stats)
}

#[derive(Debug, Default)]
pub struct ExportStats {
    pub books_exported: usize,
    pub files_copied: usize,
    pub files_would_copy: usize,
    pub files_missing: usize,
    pub files_already_exist: usize,
    pub source_missing: usize,
}

/// Status of a file comparison between source and destination
#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    /// File exists in source, not in destination - will be copied
    New,
    /// File exists in both source and destination
    Exists,
    /// File missing from source (referenced in plist but not on disk)
    SourceMissing,
}

/// Information about a file for diff display
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub source_path: PathBuf,
    pub dest_path: PathBuf,
    pub status: FileStatus,
    pub book_title: String,
    pub author: String,
}

/// Compute the diff between source and destination for all audiobooks
pub fn compute_diff(audiobooks: &[Audiobook], source_base: &Path, dest: &Path) -> Vec<FileDiff> {
    let mut diffs = Vec::new();

    for audiobook in audiobooks {
        let dest_dir = create_audiobookshelf_path(dest, audiobook);

        for track in &audiobook.tracks {
            let source_path = remap_track_path(&track.path, source_base);
            let dest_path = dest_dir.join(&track.filename);

            let status = if !source_path.exists() {
                FileStatus::SourceMissing
            } else if dest_path.exists() {
                FileStatus::Exists
            } else {
                FileStatus::New
            };

            diffs.push(FileDiff {
                source_path,
                dest_path,
                status,
                book_title: audiobook.title.clone(),
                author: audiobook.author.clone(),
            });
        }
    }

    diffs
}

/// Display a formatted diff summary
pub fn display_diff(diffs: &[FileDiff]) {
    let new_files: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == FileStatus::New)
        .collect();
    let existing_files: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == FileStatus::Exists)
        .collect();
    let missing_files: Vec<_> = diffs
        .iter()
        .filter(|d| d.status == FileStatus::SourceMissing)
        .collect();

    // Group new files by book
    let mut books_to_add: std::collections::HashMap<String, Vec<&FileDiff>> =
        std::collections::HashMap::new();
    for diff in &new_files {
        let key = format!("{} - {}", diff.author, diff.book_title);
        books_to_add.entry(key).or_default().push(diff);
    }

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║                        DIFF SUMMARY                               ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // New files (to be added)
    if !new_files.is_empty() {
        println!("┌─────────────────────────────────────────────────────────────────┐");
        println!(
            "│ \x1b[32m+ TO ADD\x1b[0m ({} files in {} books)                              ",
            new_files.len(),
            books_to_add.len()
        );
        println!("└─────────────────────────────────────────────────────────────────┘");

        let mut sorted_books: Vec<_> = books_to_add.keys().collect();
        sorted_books.sort();

        for book_key in sorted_books.iter().take(20) {
            let files = &books_to_add[*book_key];
            println!("  \x1b[32m+\x1b[0m {} ({} files)", book_key, files.len());
        }
        if books_to_add.len() > 20 {
            println!("  ... and {} more books", books_to_add.len() - 20);
        }
        println!();
    }

    // Existing files (already in destination)
    if !existing_files.is_empty() {
        let mut existing_books: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for diff in &existing_files {
            existing_books.insert(format!("{} - {}", diff.author, diff.book_title));
        }

        println!("┌─────────────────────────────────────────────────────────────────┐");
        println!(
            "│ \x1b[33m= ALREADY EXISTS\x1b[0m ({} files in {} books)                     ",
            existing_files.len(),
            existing_books.len()
        );
        println!("└─────────────────────────────────────────────────────────────────┘");

        let mut sorted_books: Vec<_> = existing_books.iter().collect();
        sorted_books.sort();

        for book_key in sorted_books.iter().take(10) {
            println!("  \x1b[33m=\x1b[0m {}", book_key);
        }
        if existing_books.len() > 10 {
            println!("  ... and {} more books", existing_books.len() - 10);
        }
        println!();
    }

    // Missing source files
    if !missing_files.is_empty() {
        let mut missing_books: std::collections::HashSet<String> = std::collections::HashSet::new();
        for diff in &missing_files {
            missing_books.insert(format!("{} - {}", diff.author, diff.book_title));
        }

        println!("┌─────────────────────────────────────────────────────────────────┐");
        println!(
            "│ \x1b[31m! SOURCE MISSING\x1b[0m ({} files in {} books)                     ",
            missing_files.len(),
            missing_books.len()
        );
        println!("└─────────────────────────────────────────────────────────────────┘");

        let mut sorted_books: Vec<_> = missing_books.iter().collect();
        sorted_books.sort();

        for book_key in sorted_books.iter().take(10) {
            println!("  \x1b[31m!\x1b[0m {}", book_key);
        }
        if missing_books.len() > 10 {
            println!("  ... and {} more books", missing_books.len() - 10);
        }
        println!();
    }

    // Summary
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ TOTALS                                                          │");
    println!("├─────────────────────────────────────────────────────────────────┤");
    println!(
        "│  \x1b[32m+\x1b[0m New files to copy:     {:>6}                               │",
        new_files.len()
    );
    println!(
        "│  \x1b[33m=\x1b[0m Already exist (skip):  {:>6}                               │",
        existing_files.len()
    );
    println!(
        "│  \x1b[31m!\x1b[0m Source missing:        {:>6}                               │",
        missing_files.len()
    );
    println!("└─────────────────────────────────────────────────────────────────┘");
}

fn main() -> Result<()> {
    let args = Args::parse();

    let source_base = args.source.unwrap_or_else(default_apple_books_path);
    let plist_path = source_base.join("Books.plist");

    println!("Reading audiobook library from: {:?}", plist_path);

    let audiobooks = parse_books_plist(&plist_path)?;

    println!("Found {} audiobooks", audiobooks.len());

    if args.dry_run {
        println!("\n=== DRY RUN - No files will be copied ===");

        // Compute and display diff
        let diffs = compute_diff(&audiobooks, &source_base, &args.dest);
        display_diff(&diffs);

        return Ok(());
    }

    let stats = export_audiobooks(&audiobooks, &source_base, &args.dest, false, args.symlink)?;

    println!("\n=== Export Summary ===");
    println!("Audiobooks processed: {}", stats.books_exported);
    println!("Files copied: {}", stats.files_copied);
    if stats.files_already_exist > 0 {
        println!(
            "Files skipped (already exist): {}",
            stats.files_already_exist
        );
    }
    if stats.files_missing > 0 {
        println!("Files missing (skipped): {}", stats.files_missing);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Normal Title"), "Normal Title");
        assert_eq!(sanitize_filename("Title: Subtitle"), "Title_ Subtitle");
        assert_eq!(sanitize_filename("What/Why"), "What_Why");
        assert_eq!(
            sanitize_filename("File<>Name*With?Bad|Chars"),
            "File__Name_With_Bad_Chars"
        );
        assert_eq!(sanitize_filename("  Trimmed  "), "Trimmed");
    }

    #[test]
    fn test_create_audiobookshelf_path() {
        let dest = PathBuf::from("/dest");

        let book_without_narrator = Audiobook {
            title: "The Great Book".to_string(),
            author: "John Doe".to_string(),
            narrator: None,
            folder_id: "sha1-abc123".to_string(),
            tracks: vec![],
        };

        let path = create_audiobookshelf_path(&dest, &book_without_narrator);
        assert_eq!(path, PathBuf::from("/dest/John Doe/The Great Book"));

        let book_with_narrator = Audiobook {
            title: "Another Book".to_string(),
            author: "Jane Smith".to_string(),
            narrator: Some("Bob Reader".to_string()),
            folder_id: "sha1-def456".to_string(),
            tracks: vec![],
        };

        let path = create_audiobookshelf_path(&dest, &book_with_narrator);
        assert_eq!(
            path,
            PathBuf::from("/dest/Jane Smith/Another Book {Bob Reader}")
        );
    }

    #[test]
    fn test_create_audiobookshelf_path_with_special_chars() {
        let dest = PathBuf::from("/dest");

        let book = Audiobook {
            title: "Book: A Subtitle".to_string(),
            author: "Author/Writer".to_string(),
            narrator: Some("Narrator: The Voice".to_string()),
            folder_id: "sha1-abc123".to_string(),
            tracks: vec![],
        };

        let path = create_audiobookshelf_path(&dest, &book);
        assert_eq!(
            path,
            PathBuf::from("/dest/Author_Writer/Book_ A Subtitle {Narrator_ The Voice}")
        );
    }

    #[test]
    fn test_remap_track_path() {
        let source_base = PathBuf::from("/Volumes/charlie/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books");

        let original_path = PathBuf::from(
            "/Users/charlie/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books/Audiobooks/sha1-abc123/01 Track.mp3"
        );

        let remapped = remap_track_path(&original_path, &source_base);

        assert_eq!(
            remapped,
            PathBuf::from("/Volumes/charlie/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books/Audiobooks/sha1-abc123/01 Track.mp3")
        );
    }

    #[test]
    fn test_parse_audiobook_entry_non_audiobook() {
        let mut dict = plist::Dictionary::new();
        dict.insert("BKBookType".to_string(), Value::String("ebook".to_string()));

        let value = Value::Dictionary(dict);
        let result = parse_audiobook_entry(&value).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_audiobook_entry_valid() {
        let mut track1 = plist::Dictionary::new();
        track1.insert(
            "itemName".to_string(),
            Value::String("Test Book".to_string()),
        );
        track1.insert("BKTrackNumber".to_string(), Value::Integer(1.into()));
        track1.insert("BKDiscNumber".to_string(), Value::Integer(0.into()));
        track1.insert(
            "BKTrackTitle".to_string(),
            Value::String("Chapter 1".to_string()),
        );
        track1.insert(
            "path".to_string(),
            Value::String("/path/to/track1.mp3".to_string()),
        );

        let mut track2 = plist::Dictionary::new();
        track2.insert(
            "itemName".to_string(),
            Value::String("Test Book".to_string()),
        );
        track2.insert("BKTrackNumber".to_string(), Value::Integer(2.into()));
        track2.insert("BKDiscNumber".to_string(), Value::Integer(0.into()));
        track2.insert(
            "BKTrackTitle".to_string(),
            Value::String("Chapter 2".to_string()),
        );
        track2.insert(
            "path".to_string(),
            Value::String("/path/to/track2.mp3".to_string()),
        );

        let parts = vec![Value::Dictionary(track1), Value::Dictionary(track2)];

        let mut dict = plist::Dictionary::new();
        dict.insert(
            "BKBookType".to_string(),
            Value::String("audiobook".to_string()),
        );
        dict.insert(
            "BKGeneratedItemId".to_string(),
            Value::String("sha1-abc123".to_string()),
        );
        dict.insert(
            "artistName".to_string(),
            Value::String("Test Author".to_string()),
        );
        dict.insert("BKParts".to_string(), Value::Array(parts));

        let value = Value::Dictionary(dict);
        let result = parse_audiobook_entry(&value).unwrap();

        assert!(result.is_some());
        let audiobook = result.unwrap();

        assert_eq!(audiobook.title, "Test Book");
        assert_eq!(audiobook.author, "Test Author");
        assert_eq!(audiobook.folder_id, "sha1-abc123");
        assert_eq!(audiobook.tracks.len(), 2);
        assert_eq!(audiobook.tracks[0].track_number, 1);
        assert_eq!(audiobook.tracks[1].track_number, 2);
    }

    #[test]
    fn test_export_creates_directory_structure() {
        let temp_source = tempdir().unwrap();
        let temp_dest = tempdir().unwrap();

        // Create a mock audiobook folder with a track
        // Source base is like: /path/to/iBooks/Books
        let audiobook_dir = temp_source.path().join("Audiobooks/sha1-test123");
        fs::create_dir_all(&audiobook_dir).unwrap();

        let track_file = audiobook_dir.join("01 Chapter 1.mp3");
        let mut file = File::create(&track_file).unwrap();
        file.write_all(b"fake audio data").unwrap();

        let audiobook = Audiobook {
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
            narrator: None,
            folder_id: "sha1-test123".to_string(),
            tracks: vec![AudioTrack {
                track_number: 1,
                disc_number: 0,
                title: "Chapter 1".to_string(),
                path: PathBuf::from("/Users/charlie/Library/Containers/com.apple.BKAgentService/Data/Documents/iBooks/Books/Audiobooks/sha1-test123/01 Chapter 1.mp3"),
                filename: "01 Chapter 1.mp3".to_string(),
            }],
        };

        let stats = export_audiobooks(
            &[audiobook],
            temp_source.path(),
            temp_dest.path(),
            false,
            false,
        )
        .unwrap();

        assert_eq!(stats.books_exported, 1);
        assert_eq!(stats.files_copied, 1);

        // Check the directory structure was created
        let expected_dir = temp_dest.path().join("Test Author/Test Book");
        assert!(expected_dir.exists());

        let expected_file = expected_dir.join("01 Chapter 1.mp3");
        assert!(expected_file.exists());
    }

    #[test]
    fn test_dry_run_does_not_copy() {
        let temp_source = tempdir().unwrap();
        let temp_dest = tempdir().unwrap();

        let audiobook = Audiobook {
            title: "Dry Run Book".to_string(),
            author: "Dry Run Author".to_string(),
            narrator: None,
            folder_id: "sha1-dryrun".to_string(),
            tracks: vec![AudioTrack {
                track_number: 1,
                disc_number: 0,
                title: "Chapter 1".to_string(),
                path: PathBuf::from("/fake/path/track.mp3"),
                filename: "track.mp3".to_string(),
            }],
        };

        let stats = export_audiobooks(
            &[audiobook],
            temp_source.path(),
            temp_dest.path(),
            true, // dry_run = true
            false,
        )
        .unwrap();

        assert_eq!(stats.files_would_copy, 1);
        assert_eq!(stats.files_copied, 0);

        // Directory should NOT be created in dry run
        let expected_dir = temp_dest.path().join("Dry Run Author/Dry Run Book");
        assert!(!expected_dir.exists());
    }
}
