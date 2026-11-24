use std::{
    collections::HashSet,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use crate::i18n::runtime::RuntimeText;
use moeka_fs::local::native as fs;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_CONFIG_FILES: usize = 128;
const MAX_INCLUDE_ENTRIES: usize = 8192;
const MAX_HOSTS: usize = 4096;

pub(crate) fn configured_hosts() -> io::Result<Vec<String>> {
    let home = super::paths::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, RuntimeText::SshHomeUnavailable))?;
    read_hosts(&home.join(".ssh/config"), &home)
}

fn read_hosts(config: &Path, home: &Path) -> io::Result<Vec<String>> {
    let mut reader = ConfigReader {
        home,
        visited: HashSet::new(),
        hosts: Vec::new(),
        aliases: HashSet::new(),
        entries: 0,
    };
    reader.read(config)?;
    Ok(reader.hosts)
}

struct ConfigReader<'a> {
    home: &'a Path,
    visited: HashSet<PathBuf>,
    hosts: Vec<String>,
    aliases: HashSet<String>,
    entries: usize,
}

impl ConfigReader<'_> {
    fn read(&mut self, path: &Path) -> io::Result<()> {
        let result = self.read_file(path);
        result.map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
    }

    fn read_file(&mut self, path: &Path) -> io::Result<()> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let canonical = fs::canonicalize(path)?;
        if self.visited.contains(&canonical) {
            return Ok(());
        }
        if self.visited.len() >= MAX_CONFIG_FILES {
            return Err(io::Error::other(RuntimeText::SshConfigIncludeLimit));
        }
        self.visited.insert(canonical);
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(io::Error::other(RuntimeText::SshConfigSizeLimit));
        }
        let source = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        for line in source.lines() {
            let tokens = config_tokens(line)?;
            let Some((directive, arguments)) = tokens.split_first() else {
                continue;
            };
            if directive.eq_ignore_ascii_case("host") {
                for alias in arguments {
                    if !alias.is_empty()
                        && !alias.starts_with(['!', '-'])
                        && !alias.contains(['*', '?'])
                        && !alias
                            .chars()
                            .any(|ch| ch.is_whitespace() || ch.is_control())
                        && self.aliases.insert(alias.to_lowercase())
                    {
                        if self.hosts.len() >= MAX_HOSTS {
                            return Err(io::Error::other(RuntimeText::SshConfigHostLimit));
                        }
                        self.hosts.push(alias.clone());
                    }
                }
            } else if directive.eq_ignore_ascii_case("include") {
                for pattern in arguments {
                    let path = if pattern == "~" {
                        self.home.to_path_buf()
                    } else if let Some(relative) = pattern.strip_prefix("~/") {
                        self.home.join(relative)
                    } else {
                        let path = PathBuf::from(pattern);
                        if path.is_absolute() {
                            path
                        } else {
                            // OpenSSH resolves user-config Includes against ~/.ssh.
                            self.home.join(".ssh").join(path)
                        }
                    };
                    for path in self.expand_include(&path)? {
                        self.read(&path)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn expand_include(&mut self, pattern: &Path) -> io::Result<Vec<PathBuf>> {
        let mut paths = vec![PathBuf::new()];
        for component in pattern.components() {
            let Component::Normal(name) = component else {
                for path in &mut paths {
                    path.push(component.as_os_str());
                }
                continue;
            };
            let name = name.to_string_lossy();
            if !name.contains(['*', '?', '[']) {
                for path in &mut paths {
                    path.push(component.as_os_str());
                }
                continue;
            }
            let matcher = glob::Pattern::new(&name)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let mut matches = Vec::new();
            for path in paths {
                let entries = match fs::read_dir(&path) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                for entry in entries {
                    self.entries += 1;
                    if self.entries > MAX_INCLUDE_ENTRIES {
                        return Err(io::Error::other(RuntimeText::SshConfigIncludeLimit));
                    }
                    let entry = entry?;
                    if matcher.matches_path(Path::new(&entry.file_name())) {
                        matches.push(entry.path());
                    }
                }
            }
            matches.sort();
            paths = matches;
        }
        Ok(paths)
    }
}

fn config_tokens(line: &str) -> io::Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            token.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_some() {
            token.push(ch);
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '#' {
            break;
        } else if ch.is_whitespace()
            || (ch == '=' && (tokens.is_empty() || (tokens.len() == 1 && token.is_empty())))
        {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if escaped || quote.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            RuntimeText::SshConfigSyntaxInvalid,
        ));
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_hosts_include_aliases_and_nested_includes_without_executing_match() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        fs::create_dir_all(ssh.join("config.d")).unwrap();
        fs::write(
            ssh.join("config"),
            "Host=alpha beta *.wild !excluded\nHostName not-an-alias\n\
             Include \"config.d/*.conf\"\nHost ALPHA \"quoted-host\" # comment\n\
             Match exec \"never execute this\"\nHost gamma\n",
        )
        .unwrap();
        fs::write(
            ssh.join("config.d/a.conf"),
            "hOsT included\nInclude ~/shared.conf\n",
        )
        .unwrap();
        fs::write(
            home.path().join("shared.conf"),
            "Host nested\nInclude config\n",
        )
        .unwrap();
        assert_eq!(
            read_hosts(&ssh.join("config"), home.path()).unwrap(),
            [
                "alpha",
                "beta",
                "included",
                "nested",
                "quoted-host",
                "gamma"
            ]
        );
    }

    #[test]
    fn ssh_hosts_missing_config_is_empty_but_invalid_config_is_reported() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("config");
        assert!(read_hosts(&config, home.path()).unwrap().is_empty());
        fs::write(&config, "Host \"unterminated\n").unwrap();
        let error = read_hosts(&config, home.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(&config.display().to_string()));
        fs::write(&config, vec![b' '; MAX_CONFIG_BYTES as usize + 1]).unwrap();
        assert!(read_hosts(&config, home.path()).is_err());
    }
}
