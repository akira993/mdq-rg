use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub hidden: bool,
    pub usage_log: bool,
    pub roots: Vec<String>,
    pub exclude: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hidden: false,
            usage_log: true,
            roots: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("mdrg.toml");
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let config: Self = toml::from_str(&source).map_err(|error: toml::de::Error| {
            let mut message = format!("{}: TOML parse error", path.display());
            if let Some(span) = error.span() {
                let (line, column) = source
                    .char_indices()
                    .take_while(|(index, _)| *index < span.start)
                    .fold((1, 1), |(line, column), (_, ch)| {
                        if ch == '\n' {
                            (line + 1, 1)
                        } else {
                            (line, column + 1)
                        }
                    });
                message.push_str(&format!(" at line {line}, column {column}"));
            }
            message
        })?;
        let base = dir
            .canonicalize()
            .map_err(|error| format!("{}: {error}", dir.display()))?;
        for name in &config.roots {
            let named = Path::new(name);
            if name.is_empty() || named.is_absolute() || crate::walk::forbidden(named) {
                return Err(format!("{}: invalid root {name:?}", path.display()));
            }
            let resolved = dir
                .join(named)
                .canonicalize()
                .map_err(|error| format!("{}: invalid root {name:?}: {error}", path.display()))?;
            if !resolved.starts_with(&base)
                || resolved
                    .strip_prefix(&base)
                    .is_ok_and(crate::walk::forbidden)
            {
                return Err(format!(
                    "{}: root {name:?} is outside the allowed directories",
                    path.display()
                ));
            }
        }
        Ok(config)
    }
}
