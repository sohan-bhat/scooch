use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "desktop.json";

pub trait Settings {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: Option<&str>) -> Result<()>;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub settings: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Set,
    Restored,
}

pub fn file(folder: &Path) -> PathBuf {
    folder.join(FILE)
}

pub fn load(path: &Path) -> Record {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(path: &Path, record: &Record) -> Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).with_context(|| format!("Scooch could not make {}, so check the disk.", folder.display()))?;
    }
    crate::state::write_atomic(path, &serde_json::to_vec_pretty(record)?)
}

pub fn write(settings: &dyn Settings, path: &Path, wanted: &[(String, String)], in_effect: &str, original: impl Fn(Option<&str>) -> bool) -> Result<Outcome> {
    let mut record = load(path);
    if let Some(old) = record.settings.get(in_effect) {
        if original(old.as_deref()) {
            restore(settings, path, &record)?;
            return Ok(Outcome::Restored);
        }
    }
    let mut changes = Vec::new();
    for (key, value) in wanted {
        let current = settings.get(key)?;
        if current.as_deref() == Some(value.as_str()) {
            continue;
        }
        record.settings.entry(key.clone()).or_insert(current);
        changes.push((key, value));
    }
    if changes.is_empty() {
        return Ok(Outcome::Set);
    }
    save(path, &record)?;
    for (key, value) in changes {
        settings.set(key, Some(value))?;
    }
    Ok(Outcome::Set)
}

pub fn restore(settings: &dyn Settings, path: &Path, record: &Record) -> Result<()> {
    for (key, value) in &record.settings {
        settings.set(key, value.as_deref())?;
    }
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error).with_context(|| format!("Scooch could not remove {}, so delete it by hand.", path.display())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        values: RefCell<BTreeMap<String, String>>,
        writes: RefCell<Vec<String>>,
    }

    impl Fake {
        fn with(pairs: &[(&str, &str)]) -> Self {
            let fake = Fake::default();
            for (key, value) in pairs {
                fake.values.borrow_mut().insert(key.to_string(), value.to_string());
            }
            fake
        }

        fn value(&self, key: &str) -> Option<String> {
            self.values.borrow().get(key).cloned()
        }
    }

    impl Settings for Fake {
        fn get(&self, key: &str) -> Result<Option<String>> {
            Ok(self.value(key))
        }

        fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
            self.writes.borrow_mut().push(key.to_string());
            match value {
                Some(value) => self.values.borrow_mut().insert(key.to_string(), value.to_string()),
                None => self.values.borrow_mut().remove(key),
            };
            Ok(())
        }
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_first_write_records_the_old_values_once() {
        let folder = crate::state::scratch("record-first");
        let path = file(&folder);
        let fake = Fake::with(&[("uri", "'old.jpg'"), ("dark", "'old-dark.jpg'"), ("options", "'zoom'")]);
        let wanted = pairs(&[("uri", "'out-1.jpg'"), ("dark", "'out-1.jpg'"), ("options", "'zoom'")]);
        assert_eq!(write(&fake, &path, &wanted, "uri", |old| old == Some("'mine.jpg'")).unwrap(), Outcome::Set);
        let record = load(&path);
        assert_eq!(record.settings.len(), 2);
        assert_eq!(record.settings["uri"].as_deref(), Some("'old.jpg'"));
        assert!(!record.settings.contains_key("options"));
        let wanted = pairs(&[("uri", "'out-2.jpg'"), ("dark", "'out-2.jpg'")]);
        write(&fake, &path, &wanted, "uri", |_| false).unwrap();
        assert_eq!(load(&path).settings["uri"].as_deref(), Some("'old.jpg'"));
        assert_eq!(fake.value("uri").as_deref(), Some("'out-2.jpg'"));
    }

    #[test]
    fn writing_the_old_picture_back_restores_everything_and_drops_the_record() {
        let folder = crate::state::scratch("record-restore");
        let path = file(&folder);
        let fake = Fake::with(&[("uri", "'old.jpg'"), ("dark", "'old-dark.jpg'"), ("options", "'spanned'")]);
        write(&fake, &path, &pairs(&[("uri", "'out.jpg'"), ("dark", "'out.jpg'"), ("options", "'zoom'")]), "uri", |_| false).unwrap();
        let outcome = write(&fake, &path, &pairs(&[("uri", "'old.jpg'"), ("dark", "'old.jpg'")]), "uri", |old| old == Some("'old.jpg'")).unwrap();
        assert_eq!(outcome, Outcome::Restored);
        assert_eq!(fake.value("uri").as_deref(), Some("'old.jpg'"));
        assert_eq!(fake.value("dark").as_deref(), Some("'old-dark.jpg'"));
        assert_eq!(fake.value("options").as_deref(), Some("'spanned'"));
        assert!(!path.exists());
    }

    #[test]
    fn a_setting_that_was_missing_is_removed_again() {
        let folder = crate::state::scratch("record-missing");
        let path = file(&folder);
        let fake = Fake::with(&[("image", "/p/old.png")]);
        write(&fake, &path, &pairs(&[("image", "/out.png"), ("style", "5")]), "image", |_| false).unwrap();
        assert_eq!(load(&path).settings["style"], None);
        write(&fake, &path, &pairs(&[("image", "/p/old.png")]), "image", |old| old == Some("/p/old.png")).unwrap();
        assert_eq!(fake.value("style"), None);
        assert_eq!(fake.value("image").as_deref(), Some("/p/old.png"));
    }

    #[test]
    fn nothing_is_recorded_or_written_when_the_values_already_match() {
        let folder = crate::state::scratch("record-same");
        let path = file(&folder);
        let fake = Fake::with(&[("uri", "'same.jpg'")]);
        write(&fake, &path, &pairs(&[("uri", "'same.jpg'")]), "uri", |_| false).unwrap();
        assert!(!path.exists());
        assert!(fake.writes.borrow().is_empty());
    }

    #[test]
    fn the_record_is_on_disk_before_any_setting_changes() {
        struct Watching<'a> {
            inner: Fake,
            path: &'a Path,
        }
        impl Settings for Watching<'_> {
            fn get(&self, key: &str) -> Result<Option<String>> {
                self.inner.get(key)
            }
            fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
                assert!(self.path.exists());
                self.inner.set(key, value)
            }
        }
        let folder = crate::state::scratch("record-order");
        let path = file(&folder);
        let watching = Watching { inner: Fake::with(&[("uri", "'old.jpg'")]), path: &path };
        write(&watching, &path, &pairs(&[("uri", "'new.jpg'")]), "uri", |_| false).unwrap();
    }
}
