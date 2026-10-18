//! Per-instance Git exclude blocks. Call while holding the registry write lock.
use crate::{
    error::Error,
    services::{fs::Fs, git::Git},
};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> Error {
    Error::Registry {
        message: message.into(),
        hint: "Repair the skillctl exclude markers before retrying.".into(),
    }
}
fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation: format!("Update Git exclude {}", path.display()),
        source,
        hint: "Check repository permissions and retry skillctl relink.".into(),
    }
}

/// Replace only this instance's block, preserving all other bytes. Empty paths
/// remove the block. Malformed markers fail closed instead of deleting user data.
pub fn rewrite(
    original: &[u8],
    instance: &Path,
    patterns: &BTreeSet<String>,
) -> Result<Vec<u8>, Error> {
    let key = instance
        .to_str()
        .ok_or_else(|| invalid("Instance path is not UTF-8"))?;
    if key.contains(['\r', '\n']) {
        return Err(invalid("Instance path contains a line break"));
    }
    let start = format!("# >>> skillctl {key} >>>");
    let end = format!("# <<< skillctl {key} <<<");
    let mut begin = None;
    let mut range = None;
    let mut offset = 0;
    for line in original.split_inclusive(|b| *b == b'\n') {
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        if content == start.as_bytes() {
            if begin.is_some() || range.is_some() {
                return Err(invalid("Duplicate or nested skillctl exclude markers"));
            }
            begin = Some(offset);
        } else if content == end.as_bytes() {
            let first = begin
                .take()
                .ok_or_else(|| invalid("Unmatched skillctl exclude end marker"))?;
            range = Some(first..offset + line.len());
        } else if begin.is_some() && content.starts_with(b"# >>> skillctl ") {
            return Err(invalid("Nested skillctl exclude blocks"));
        }
        offset += line.len();
    }
    if begin.is_some() {
        return Err(invalid("Unclosed skillctl exclude block"));
    }
    let newline = if original.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut block = Vec::new();
    if !patterns.is_empty() {
        block.extend_from_slice(start.as_bytes());
        block.extend_from_slice(newline.as_bytes());
        for pattern in patterns {
            if !pattern.starts_with('/') || pattern.contains(['\r', '\n']) {
                return Err(invalid("Invalid anchored Git exclude pattern"));
            }
            block.extend_from_slice(pattern.as_bytes());
            block.extend_from_slice(newline.as_bytes());
        }
        block.extend_from_slice(end.as_bytes());
        block.extend_from_slice(newline.as_bytes());
    }
    let mut result = Vec::new();
    if let Some(range) = range {
        result.extend_from_slice(&original[..range.start]);
        result.extend_from_slice(&block);
        result.extend_from_slice(&original[range.end..]);
    } else {
        result.extend_from_slice(original);
        if !block.is_empty() && !result.is_empty() && !result.ends_with(b"\n") {
            result.extend_from_slice(newline.as_bytes());
        }
        result.extend_from_slice(&block);
    }
    Ok(result)
}

/// Gitignore metacharacters must be literal; otherwise a link named within a
/// bracket-containing instance could accidentally hide project-owned files.
fn pattern(root: &Path, path: &Path) -> Result<String, Error> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("Skill link lies outside the Git worktree"))?;
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(invalid("Skill link is not a normalized relative path"));
    }
    let value = relative
        .to_str()
        .ok_or_else(|| invalid("Skill link path is not UTF-8"))?;
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(invalid("Invalid skill link path"));
    }
    let mut result = String::from("/");
    for c in value.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | ' ' | '#' | '!') {
            result.push('\\');
        }
        result.push(c);
    }
    Ok(result)
}

/// Hide successfully managed links, never entire target directories. Pass an
/// empty iterator on detach. Outside a worktree this is a silent no-op.
/// Returns the exclude file used. Writes are atomic and omitted if unchanged.
pub fn update(
    fs: &dyn Fs,
    git: &dyn Git,
    instance: &Path,
    links: impl IntoIterator<Item = PathBuf>,
) -> Result<Option<PathBuf>, Error> {
    let Some(root) = git.toplevel(instance)? else {
        return Ok(None);
    };
    let Some(file) = git.exclude_file(instance)? else {
        return Ok(None);
    };
    let patterns = links
        .into_iter()
        .map(|path| pattern(&root, &path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let original = match fs.read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(io_error(&file, e)),
    };
    let changed = rewrite(&original, instance, &patterns)?;
    if changed != original {
        let mut missing = Vec::new();
        let mut parent = file.parent();
        while let Some(path) = parent {
            match fs.symlink_metadata(path) {
                Ok(metadata) if metadata.is_dir() => break,
                Ok(_) => {
                    return Err(invalid(format!(
                        "Exclude parent is not a directory: {}",
                        path.display()
                    )));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => missing.push(path.to_path_buf()),
                Err(e) => return Err(io_error(path, e)),
            }
            parent = path.parent();
        }
        for path in missing.iter().rev() {
            fs.create_dir(path).map_err(|e| io_error(path, e))?;
        }
        fs.write_atomic(&file, &changed)
            .map_err(|e| io_error(&file, e))?;
    }
    Ok(Some(file))
}
