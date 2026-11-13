use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "desktop.json";

pub trait Settings {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: Option<&str>) -> Result<()>;
}

pub type Saved = BTreeMap<String, Option<String>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub desktops: BTreeMap<String, Saved>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Set,
    Restored,
}

pub struct Request<'a> {
    pub desktop: &'a str,
    pub wanted: &'a [(String, String)],
    pub pictures: &'a [String],
    pub target_is_ours: bool,
}

pub fn file(folder: &Path) -> PathBuf {
    folder.join(FILE)
}

pub fn load(path: &Path) -> Record {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(path: &Path, record: &Record) -> Result<()> {
    if record.desktops.values().all(BTreeMap::is_empty) {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error).with_context(|| format!("Scooch could not remove {}, so delete it by hand.", path.display())),
            _ => Ok(()),
        };
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).with_context(|| format!("Scooch could not make {}, so check the disk.", folder.display()))?;
    }
    crate::state::write_atomic(path, &serde_json::to_vec_pretty(record)?)
}

pub fn should_restore(settings: &dyn Settings, saved: &Saved, request: &Request, original: &dyn Fn(Option<&str>) -> bool, ours: &dyn Fn(Option<&str>) -> bool) -> Result<bool> {
    let recorded: Vec<&String> = request.pictures.iter().filter(|key| saved.contains_key(*key)).collect();
    if recorded.is_empty() {
        return Ok(false);
    }
    if recorded.iter().any(|key| original(saved[*key].as_deref())) {
        return Ok(true);
    }
    if request.target_is_ours {
        return Ok(false);
    }
    for key in recorded {
        if !ours(settings.get(key)?.as_deref()) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn write(settings: &dyn Settings, path: &Path, request: &Request, original: impl Fn(Option<&str>) -> bool, ours: impl Fn(Option<&str>) -> bool) -> Result<Outcome> {
    let mut record = load(path);
    let mut saved = record.desktops.get(request.desktop).cloned().unwrap_or_default();
    if should_restore(settings, &saved, request, &original, &ours)? {
        restore(settings, path, request.desktop)?;
        return Ok(Outcome::Restored);
    }
    let mut changes = Vec::new();
    for (key, value) in request.wanted {
        let current = settings.get(key)?;
        if current.as_deref() == Some(value.as_str()) {
            continue;
        }
        if !saved.contains_key(key) && !ours(current.as_deref()) {
            saved.insert(key.clone(), current.clone());
        }
        changes.push((key, value, current));
    }
    if changes.is_empty() {
        return Ok(Outcome::Set);
    }
    record.desktops.insert(request.desktop.to_string(), saved);
    save(path, &record)?;
    for (index, (key, value, _)) in changes.iter().enumerate() {
        if let Err(error) = settings.set(key, Some(value)) {
            for (earlier, _, before) in changes[..index].iter().rev() {
                let _ = settings.set(earlier, before.as_deref());
            }
            return Err(error);
        }
    }
    Ok(Outcome::Set)
}

pub fn restore(settings: &dyn Settings, path: &Path, desktop: &str) -> Result<()> {
    let mut record = load(path);
    let Some(saved) = record.desktops.get(desktop).cloned() else { return Ok(()) };
    for (key, value) in &saved {
        settings.set(key, value.as_deref())?;
    }
    record.desktops.remove(desktop);
    save(path, &record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        values: RefCell<BTreeMap<String, String>>,
        writes: RefCell<Vec<String>>,
        broken: Option<String>,
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
            if self.broken.as_deref() == Some(key) {
                anyhow::bail!("no such key {key}");
            }
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

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|key| key.to_string()).collect()
    }

    fn ours(value: Option<&str>) -> bool {
        value.is_some_and(|value| value.contains("/out/"))
    }

    fn set(fake: &Fake, path: &Path, desktop: &str, wanted: &[(&str, &str)], pictures: &[&str], target: &str) -> Result<Outcome> {
        let wanted = pairs(wanted);
        let pictures = keys(pictures);
        let request = Request { desktop, wanted: &wanted, pictures: &pictures, target_is_ours: ours(Some(target)) };
        write(fake, path, &request, |old| old == Some(target), ours)
    }

    #[test]
    fn the_first_write_records_the_old_values_once() {
        let path = file(&crate::state::scratch("record-first"));
        let fake = Fake::with(&[("uri", "old.jpg"), ("dark", "old-dark.jpg"), ("options", "zoom")]);
        let outcome = set(&fake, &path, "gnome", &[("uri", "/d/out/1.jpg"), ("dark", "/d/out/1.jpg"), ("options", "zoom")], &["uri", "dark"], "/d/out/1.jpg").unwrap();
        assert_eq!(outcome, Outcome::Set);
        let saved = &load(&path).desktops["gnome"];
        assert_eq!(saved.len(), 2);
        assert_eq!(saved["uri"].as_deref(), Some("old.jpg"));
        set(&fake, &path, "gnome", &[("uri", "/d/out/2.jpg"), ("dark", "/d/out/2.jpg")], &["uri", "dark"], "/d/out/2.jpg").unwrap();
        assert_eq!(load(&path).desktops["gnome"]["uri"].as_deref(), Some("old.jpg"));
        assert_eq!(fake.value("uri").as_deref(), Some("/d/out/2.jpg"));
    }

    #[test]
    fn setting_any_recorded_picture_restores_everything() {
        let path = file(&crate::state::scratch("record-any"));
        let fake = Fake::with(&[("uri", "light.jpg"), ("dark", "dark.jpg"), ("options", "spanned")]);
        set(&fake, &path, "gnome", &[("uri", "/d/out/1.jpg"), ("dark", "/d/out/1.jpg"), ("options", "zoom")], &["uri", "dark"], "/d/out/1.jpg").unwrap();
        let outcome = set(&fake, &path, "gnome", &[("uri", "dark.jpg"), ("dark", "dark.jpg")], &["uri", "dark"], "dark.jpg").unwrap();
        assert_eq!(outcome, Outcome::Restored);
        assert_eq!((fake.value("uri").as_deref(), fake.value("dark").as_deref(), fake.value("options").as_deref()), (Some("light.jpg"), Some("dark.jpg"), Some("spanned")));
        assert!(!path.exists());
    }

    #[test]
    fn outputs_in_every_picture_key_restore_on_any_outside_picture() {
        let path = file(&crate::state::scratch("record-ours"));
        let fake = Fake::with(&[("uri", "light.jpg"), ("dark", "dark.jpg")]);
        set(&fake, &path, "gnome", &[("uri", "/d/out/1.jpg"), ("dark", "/d/out/1.jpg")], &["uri", "dark"], "/d/out/1.jpg").unwrap();
        assert_eq!(set(&fake, &path, "gnome", &[("uri", "/d/out/2.jpg")], &["uri", "dark"], "/d/out/2.jpg").unwrap(), Outcome::Set);
        let outcome = set(&fake, &path, "gnome", &[("uri", "elsewhere.jpg")], &["uri", "dark"], "elsewhere.jpg").unwrap();
        assert_eq!(outcome, Outcome::Restored);
        assert_eq!(fake.value("dark").as_deref(), Some("dark.jpg"));
    }

    #[test]
    fn an_output_is_never_recorded_as_an_old_value() {
        let path = file(&crate::state::scratch("record-guard"));
        let fake = Fake::with(&[("image", "/d/out/7.jpg"), ("style", "2")]);
        set(&fake, &path, "kde", &[("image", "/d/out/8.jpg"), ("style", "5")], &["image"], "/d/out/8.jpg").unwrap();
        let saved = &load(&path).desktops["kde"];
        assert!(!saved.contains_key("image"));
        assert_eq!(saved["style"].as_deref(), Some("2"));
    }

    #[test]
    fn a_failed_later_key_puts_the_earlier_keys_back() {
        let path = file(&crate::state::scratch("record-rollback"));
        let fake = Fake { broken: Some("dark".into()), ..Fake::with(&[("uri", "old.jpg")]) };
        assert!(set(&fake, &path, "gnome", &[("uri", "/d/out/1.jpg"), ("dark", "/d/out/1.jpg")], &["uri", "dark"], "/d/out/1.jpg").is_err());
        assert_eq!(fake.value("uri").as_deref(), Some("old.jpg"));
    }

    #[test]
    fn each_desktop_keeps_its_own_record() {
        let path = file(&crate::state::scratch("record-desktops"));
        let fake = Fake::with(&[("uri", "gnome.jpg"), ("image", "kde.jpg")]);
        set(&fake, &path, "gnome", &[("uri", "/d/out/1.jpg")], &["uri"], "/d/out/1.jpg").unwrap();
        set(&fake, &path, "kde", &[("image", "/d/out/2.jpg")], &["image"], "/d/out/2.jpg").unwrap();
        set(&fake, &path, "kde", &[("image", "kde.jpg")], &["image"], "kde.jpg").unwrap();
        assert_eq!(fake.value("uri").as_deref(), Some("/d/out/1.jpg"));
        let record = load(&path);
        assert!(record.desktops.contains_key("gnome") && !record.desktops.contains_key("kde"));
    }

    #[test]
    fn a_setting_that_was_missing_is_removed_again() {
        let path = file(&crate::state::scratch("record-missing"));
        let fake = Fake::with(&[("image", "/p/old.png")]);
        set(&fake, &path, "xfce", &[("image", "/d/out/1.png"), ("style", "5")], &["image"], "/d/out/1.png").unwrap();
        assert_eq!(load(&path).desktops["xfce"]["style"], None);
        set(&fake, &path, "xfce", &[("image", "/p/old.png")], &["image"], "/p/old.png").unwrap();
        assert_eq!(fake.value("style"), None);
        assert_eq!(fake.value("image").as_deref(), Some("/p/old.png"));
    }

    #[test]
    fn nothing_is_recorded_or_written_when_the_values_already_match() {
        let path = file(&crate::state::scratch("record-same"));
        let fake = Fake::with(&[("uri", "same.jpg")]);
        set(&fake, &path, "gnome", &[("uri", "same.jpg")], &["uri"], "same.jpg").unwrap();
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
        let path = file(&crate::state::scratch("record-order"));
        let watching = Watching { inner: Fake::with(&[("uri", "old.jpg")]), path: &path };
        let wanted = pairs(&[("uri", "/d/out/1.jpg")]);
        let pictures = keys(&["uri"]);
        let request = Request { desktop: "gnome", wanted: &wanted, pictures: &pictures, target_is_ours: true };
        write(&watching, &path, &request, |_| false, ours).unwrap();
    }
}
