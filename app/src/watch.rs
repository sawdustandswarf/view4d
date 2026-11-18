//! Noticing a file change: its size and time, looked at now and then, and
//! a change taken once two looks agree, so a file still being written is
//! not read half done.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// How often to look.
pub const EVERY: Duration = Duration::from_millis(500);

type Stamp = Option<(u64, SystemTime)>;

pub struct Watch {
    path: PathBuf,
    /// As it was when last read.
    loaded: Stamp,
    /// As it was at the last look.
    seen: Stamp,
}

impl Watch {
    /// Watch `path`, as read just now.
    pub fn new(path: PathBuf) -> Watch {
        let now = stamp(&path);
        Watch { path, loaded: now, seen: now }
    }

    /// Look; true if it has changed since it was read and was the same at
    /// the last look (so it is time to read it again).
    pub fn changed(&mut self) -> bool {
        let now = stamp(&self.path);
        let settled = now == self.seen;
        self.seen = now;
        if settled && now.is_some() && now != self.loaded {
            self.loaded = now;
            return true;
        }
        false
    }
}

fn stamp(path: &PathBuf) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_taken_once_it_settles() {
        let path = std::env::temp_dir().join(format!("view4d-watch-{}", std::process::id()));
        std::fs::write(&path, "one").unwrap();
        let mut w = Watch::new(path.clone());
        assert!(!w.changed(), "as read");
        std::fs::write(&path, "two!").unwrap();
        assert!(!w.changed(), "just changed: it may still be being written");
        assert!(w.changed(), "the same twice: read it");
        assert!(!w.changed(), "once");
        std::fs::remove_file(&path).unwrap();
        assert!(!w.changed() && !w.changed(), "gone: nothing to read");
        std::fs::write(&path, "three").unwrap();
        assert!(!w.changed());
        assert!(w.changed(), "back again");
        std::fs::remove_file(&path).unwrap();
    }
}
