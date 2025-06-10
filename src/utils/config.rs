mod config {
    use std::fs;
    use std::path::Path;
    use toml;

    #[derive(Debug, Deserialize)]
    pub struct Config {
        pub timeout: u64,
        pub interfaces: Vec<String>,
        pub log_level: String,
    }

    impl Config {
        pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
            let content = fs::read_to_string(path)?;
            let config: Config = toml::de::from_str(&content)?;
            Ok(config)
        }
    }
}