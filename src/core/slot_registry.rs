use anyhow::{Context, Result};
use fs2::FileExt;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SlotEntry {
    pub path: PathBuf,
    pub slot: u32,
}

#[derive(Debug)]
pub struct SlotRegistry {
    file_path: PathBuf,
    pub entries: Vec<SlotEntry>,
    /// When held, ensures exclusive access from load through save.
    _lock: Option<File>,
}

impl SlotRegistry {
    /// Load registry from the git common dir (read-only, no lock).
    /// Use this for commands that only read the registry (e.g. `env`, `status`).
    pub fn load(git_common_dir: &Path) -> Result<Self> {
        let file_path = registry_path(git_common_dir);
        let entries = if file_path.exists() {
            read_entries(&file_path)?
        } else {
            Vec::new()
        };
        Ok(SlotRegistry {
            file_path,
            entries,
            _lock: None,
        })
    }

    /// Load registry while holding an exclusive lock.
    /// The lock is held from load through save, preventing TOCTOU races.
    /// Use this for commands that do read-modify-write (e.g. `init`, `cleanup`, `prune`).
    pub fn load_locked(git_common_dir: &Path) -> Result<Self> {
        let file_path = registry_path(git_common_dir);
        let lock = lock_file(&file_path)?;
        let entries = if file_path.exists() {
            read_entries(&file_path)?
        } else {
            Vec::new()
        };
        Ok(SlotRegistry {
            file_path,
            entries,
            _lock: Some(lock),
        })
    }

    /// Save registry to disk atomically (write to temp, then rename).
    /// If loaded with `load_locked`, the lock is already held.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let tmp_path = self.file_path.with_extension("tmp");
        let mut f = File::create(&tmp_path)
            .with_context(|| format!("failed to write temp registry: {}", tmp_path.display()))?;
        for entry in &self.entries {
            writeln!(f, "{}\t{}", entry.path.display(), entry.slot)?;
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp_path, &self.file_path).with_context(|| {
            format!(
                "failed to rename temp registry to {}",
                self.file_path.display()
            )
        })?;
        Ok(())
    }

    pub fn get_slot(&self, worktree_path: &Path) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.path == worktree_path)
            .map(|e| e.slot)
    }

    /// Assign a slot for the given path. If it already has one, return it.
    /// Main worktree (slot 0) must be ensured by the caller before calling this
    /// for linked worktrees.
    pub fn assign_slot(&mut self, worktree_path: &Path, main_root: &Path) -> u32 {
        // Return existing slot
        if let Some(slot) = self.get_slot(worktree_path) {
            return slot;
        }

        // Ensure main worktree has slot 0
        if self.get_slot(main_root).is_none() {
            self.entries.push(SlotEntry {
                path: main_root.to_path_buf(),
                slot: 0,
            });
        }

        // If this IS the main worktree, return 0
        if worktree_path == main_root {
            return 0;
        }

        // Find next available slot (starting from 1)
        let used: BTreeSet<u32> = self.entries.iter().map(|e| e.slot).collect();
        let mut slot = 1u32;
        while used.contains(&slot) {
            slot += 1;
        }

        self.entries.push(SlotEntry {
            path: worktree_path.to_path_buf(),
            slot,
        });
        slot
    }

    pub fn remove(&mut self, worktree_path: &Path) {
        self.entries.retain(|e| e.path != worktree_path);
    }

    /// Remove entries whose paths no longer exist on disk.
    /// Returns the removed entries.
    pub fn prune_stale(&mut self) -> Vec<SlotEntry> {
        let mut removed = Vec::new();
        self.entries.retain(|e| {
            if e.path.exists() {
                true
            } else {
                removed.push(e.clone());
                false
            }
        });
        removed
    }
}

fn registry_path(git_common_dir: &Path) -> PathBuf {
    // git_common_dir is typically /path/to/repo/.git
    // The registry lives inside it as worktree-slots
    git_common_dir.join("worktree-slots")
}

fn read_entries(path: &Path) -> Result<Vec<SlotEntry>> {
    let file =
        File::open(path).with_context(|| format!("failed to open registry: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() != 2 {
            return Err(crate::error::Error::SlotRegistryCorrupt {
                path: path.to_path_buf(),
                reason: format!("invalid line {}: {}", i + 1, line),
            }
            .into());
        }
        let slot: u32 = parts[1]
            .parse()
            .map_err(|_| crate::error::Error::SlotRegistryCorrupt {
                path: path.to_path_buf(),
                reason: format!("invalid slot number on line {}: {}", i + 1, parts[1]),
            })?;
        entries.push(SlotEntry {
            path: PathBuf::from(parts[0]),
            slot,
        });
    }

    Ok(entries)
}

fn lock_file(registry_path: &Path) -> Result<File> {
    let lock_path = registry_path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .with_context(|| format!("failed to create lock file: {}", lock_path.display()))?;
    lock.lock_exclusive()
        .context("failed to acquire exclusive lock on slot registry")?;
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        (dir, git_dir)
    }

    #[test]
    fn test_empty_registry() {
        let (_dir, git_dir) = setup();
        let reg = SlotRegistry::load(&git_dir).unwrap();
        assert!(reg.entries.is_empty());
    }

    #[test]
    fn test_assign_main_slot() {
        let (_dir, git_dir) = setup();
        let mut reg = SlotRegistry::load(&git_dir).unwrap();
        let main_root = PathBuf::from("/tmp/main");
        let slot = reg.assign_slot(&main_root, &main_root);
        assert_eq!(slot, 0);
    }

    #[test]
    fn test_assign_linked_slot() {
        let (_dir, git_dir) = setup();
        let mut reg = SlotRegistry::load(&git_dir).unwrap();
        let main_root = PathBuf::from("/tmp/main");
        let linked = PathBuf::from("/tmp/linked1");

        reg.assign_slot(&main_root, &main_root);
        let slot = reg.assign_slot(&linked, &main_root);
        assert_eq!(slot, 1);
    }

    #[test]
    fn test_save_and_reload() {
        let (_dir, git_dir) = setup();
        let mut reg = SlotRegistry::load_locked(&git_dir).unwrap();
        let main_root = PathBuf::from("/tmp/main");
        reg.assign_slot(&main_root, &main_root);
        reg.save().unwrap();
        drop(reg); // release lock

        let reg2 = SlotRegistry::load(&git_dir).unwrap();
        assert_eq!(reg2.entries.len(), 1);
        assert_eq!(reg2.entries[0].slot, 0);
    }

    #[test]
    fn test_remove() {
        let (_dir, git_dir) = setup();
        let mut reg = SlotRegistry::load(&git_dir).unwrap();
        let main_root = PathBuf::from("/tmp/main");
        let linked = PathBuf::from("/tmp/linked1");
        reg.assign_slot(&main_root, &main_root);
        reg.assign_slot(&linked, &main_root);
        assert_eq!(reg.entries.len(), 2);

        reg.remove(&linked);
        assert_eq!(reg.entries.len(), 1);
    }

    #[test]
    fn test_slot_reuse_after_remove() {
        let (_dir, git_dir) = setup();
        let mut reg = SlotRegistry::load(&git_dir).unwrap();
        let main_root = PathBuf::from("/tmp/main");
        let linked1 = PathBuf::from("/tmp/linked1");
        let linked2 = PathBuf::from("/tmp/linked2");
        let linked3 = PathBuf::from("/tmp/linked3");

        reg.assign_slot(&main_root, &main_root);
        reg.assign_slot(&linked1, &main_root); // slot 1
        reg.assign_slot(&linked2, &main_root); // slot 2

        reg.remove(&linked1); // free slot 1
        let slot = reg.assign_slot(&linked3, &main_root);
        assert_eq!(slot, 1); // reuses slot 1
    }
}
