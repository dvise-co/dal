use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub ytdlp: YtdlpConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub download_dir: String,
    pub database: String,
    pub segment_by_host: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct YtdlpConfig {
    pub binary: String,
    pub max_concurrent: usize,
    pub output_template: Option<String>,
    pub default_args: Vec<String>,
    pub extra_args: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8550,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            download_dir: "./downloads".to_string(),
            database: "./dal.db".to_string(),
            segment_by_host: true,
        }
    }
}

impl Default for YtdlpConfig {
    fn default() -> Self {
        Self {
            binary: "yt-dlp".to_string(),
            max_concurrent: 1,
            output_template: None,
            default_args: vec![
                "--newline".to_string(),
                "--no-colors".to_string(),
                "--verbose".to_string(),
                "--progress".to_string(),
                "--continue".to_string(),
                "--trim-filenames".to_string(),
                "150".to_string(),
            ],
            extra_args: vec![],
        }
    }
}

pub fn load_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    if path.exists() {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    } else {
        tracing::info!(
            "Config file not found at {}, using defaults",
            path.display()
        );
        Ok(Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8550);
        assert_eq!(config.storage.download_dir, "./downloads");
        assert!(config.storage.segment_by_host);
        assert_eq!(config.ytdlp.max_concurrent, 1);
        assert!(config.ytdlp.output_template.is_none());
        assert_eq!(config.ytdlp.default_args.len(), 7); // --newline, --no-colors, --verbose, --progress, --continue, --trim-filenames, 150
        assert!(config.ytdlp.extra_args.is_empty());
    }

    #[test]
    fn test_parse_full_toml() {
        let toml = r#"
            [server]
            host = "127.0.0.1"
            port = 9000

            [storage]
            download_dir = "/tmp/videos"
            database = "/tmp/dal.db"
            segment_by_host = false

            [ytdlp]
            binary = "/usr/bin/yt-dlp"
            max_concurrent = 3
            default_args = ["--newline", "--continue"]
            output_template = "%(uploader)s/%(title)s.%(ext)s"
            extra_args = ["--format", "best"]
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.storage.download_dir, "/tmp/videos");
        assert!(!config.storage.segment_by_host);
        assert_eq!(config.ytdlp.max_concurrent, 3);
        assert_eq!(config.ytdlp.default_args, vec!["--newline", "--continue"]);
        assert_eq!(
            config.ytdlp.output_template.as_deref(),
            Some("%(uploader)s/%(title)s.%(ext)s")
        );
        assert_eq!(config.ytdlp.extra_args, vec!["--format", "best"]);
    }

    #[test]
    fn test_parse_partial_toml() {
        let toml = r#"
            [server]
            port = 7000
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.server.port, 7000);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.storage.download_dir, "./downloads");
    }

    #[test]
    fn test_load_missing_file() {
        let config = load_config(Path::new("/nonexistent/path.toml")).unwrap();
        assert_eq!(config.server.port, 8550);
    }
}
