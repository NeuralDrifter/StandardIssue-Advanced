// Michael Burgus — https://github.com/NeuralDrifter
// Small file helpers shared by every JSON store

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

/// Write to a sibling temp file, then rename over the target, so a crash never
/// leaves a half-written file.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(value).map_err(|e| format!("serialize: {e}"))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    write_atomic(path, &data).map_err(|e| format!("write {name}: {e}"))
}

/// Load a JSON file, falling back to defaults. A corrupt file is moved aside;
/// the returned warning says what happened.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> (T, Option<String>) {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (T::default(), None),
        Err(e) => (T::default(), Some(format!("could not read {name}: {e}"))),
        Ok(data) => match serde_json::from_str(&data) {
            Ok(v) => (v, None),
            Err(e) => {
                let aside = path.with_file_name(format!(
                    "{name}.corrupt-{}",
                    crate::instance::unix_now()
                ));
                let _ = std::fs::rename(path, &aside);
                (
                    T::default(),
                    Some(format!("{name} was corrupt ({e}); moved to {}", aside.display())),
                )
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn atomic_write_replaces_and_leaves_no_tmp() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("books.json");
        write_atomic(&path, b"[]").unwrap();
        write_atomic(&path, b"[1]").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
        assert!(!t.path().join("books.json.tmp").exists());
    }

    #[test]
    fn load_json_missing_corrupt_and_ok() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("x.json");
        let (v, w): (BTreeMap<String, u32>, _) = load_json(&path);
        assert!(v.is_empty() && w.is_none());

        std::fs::write(&path, "{ nope").unwrap();
        let (_, w): (BTreeMap<String, u32>, _) = load_json(&path);
        assert!(w.unwrap().contains("corrupt"));
        assert!(!path.exists());

        save_json(&path, &BTreeMap::from([("a".to_string(), 1u32)])).unwrap();
        let (v, _): (BTreeMap<String, u32>, _) = load_json(&path);
        assert_eq!(v["a"], 1);
    }
}
