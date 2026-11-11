//! Riot's static game data ("Data Dragon"): champion names and item prices.

use crate::gold::PriceTable;
use std::collections::HashMap;

/// Champion id -> display name.
#[derive(Debug, Clone, Default)]
pub struct ChampionNames(HashMap<i64, String>);

impl ChampionNames {
    /// Parse `…/cdn/<patch>/data/en_US/champion.json`.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let mut map = HashMap::new();
        if let Some(data) = v.get("data").and_then(|d| d.as_object()) {
            for champ in data.values() {
                let id = champ.get("key").and_then(|k| k.as_str()).and_then(|k| k.parse::<i64>().ok());
                let name = champ.get("name").and_then(|n| n.as_str());
                if let (Some(id), Some(name)) = (id, name) {
                    map.insert(id, name.to_string());
                }
            }
        }
        Ok(ChampionNames(map))
    }

    pub fn get(&self, id: i64) -> Option<&str> {
        self.0.get(&id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The newest patch in `…/api/versions.json` (a JSON array, newest first).
pub fn latest_version(json: &str) -> Option<String> {
    let v: Vec<String> = serde_json::from_str(json).ok()?;
    v.into_iter().next().filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct StaticData {
    pub version: String,
    pub champions: ChampionNames,
    pub prices: PriceTable,
}

#[cfg(feature = "client")]
mod fetch {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const BASE: &str = "https://ddragon.leagueoflegends.com";

    fn get(url: &str) -> Option<String> {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().ok()?;
        let resp = client.get(url).send().ok()?;
        resp.status().is_success().then(|| resp.text().ok()).flatten()
    }

    /// Newest patch folder already in the cache, by version number.
    fn newest_cached(cache: &Path) -> Option<String> {
        let key = |v: &str| v.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
        std::fs::read_dir(cache)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("champion.json").is_file() && e.path().join("item.json").is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .max_by_key(|v| key(v))
    }

    fn cached_or_download(dir: &Path, version: &str, file: &str) -> Option<String> {
        let path: PathBuf = dir.join(file);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Some(text);
        }
        let text = get(&format!("{BASE}/cdn/{version}/data/en_US/{file}"))?;
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, &text);
        Some(text)
    }

    /// Load static data for the current patch, downloading it into `cache`
    /// if needed. Offline, the newest cached patch is used. `None` when there
    /// is neither a connection nor a cache.
    pub fn load(cache: &Path) -> Option<StaticData> {
        let version = get(&format!("{BASE}/api/versions.json"))
            .and_then(|j| latest_version(&j))
            .or_else(|| newest_cached(cache))?;
        let dir = cache.join(&version);
        let champions = ChampionNames::from_json(&cached_or_download(&dir, &version, "champion.json")?).ok()?;
        let prices = PriceTable::from_ddragon_json(&cached_or_download(&dir, &version, "item.json")?).ok()?;
        Some(StaticData { version, champions, prices })
    }
}

#[cfg(feature = "client")]
pub use fetch::load;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn champion_names() {
        let json = r#"{"type":"champion","data":{
            "Ahri":{"id":"Ahri","key":"103","name":"Ahri"},
            "MonkeyKing":{"id":"MonkeyKing","key":"62","name":"Wukong"},
            "Broken":{"id":"Broken","key":"abc","name":"Nope"}}}"#;
        let names = ChampionNames::from_json(json).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names.get(62), Some("Wukong"));
        assert_eq!(names.get(1), None);
    }

    #[test]
    fn versions() {
        assert_eq!(latest_version(r#"["16.22.1","16.21.1","lolpatch_3.7"]"#), Some("16.22.1".into()));
        assert_eq!(latest_version("[]"), None);
        assert_eq!(latest_version("oops"), None);
    }
}
