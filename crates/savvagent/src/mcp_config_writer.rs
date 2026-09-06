#![cfg_attr(not(test), allow(dead_code))]

use std::io;
use std::path::Path;

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::config_file::{McpAuthMode, McpServerEntry};

pub fn add_server(path: &Path, entry_toml: Table) -> io::Result<()> {
    let mut document = load_document(path)?;
    mcp_servers_mut(&mut document, true)?
        .expect("create_if_missing=true must yield an array-of-tables")
        .push(entry_toml);
    write_document(path, document)
}

pub fn remove_server(path: &Path, name: &str) -> io::Result<bool> {
    let mut document = load_document(path)?;
    let Some(servers) = mcp_servers_mut(&mut document, false)? else {
        return Ok(false);
    };
    let Some(idx) = servers
        .iter()
        .position(|table| table.get("name").and_then(Item::as_str) == Some(name))
    else {
        return Ok(false);
    };
    servers.remove(idx);
    write_document(path, document)?;
    Ok(true)
}

pub fn entry_table(entry: &McpServerEntry) -> Table {
    let mut table = Table::new();
    match entry {
        McpServerEntry::Stdio {
            name,
            command,
            args,
            env,
        } => {
            table["transport"] = toml_edit::value("stdio");
            table["name"] = toml_edit::value(name.as_str());
            table["command"] = toml_edit::value(command.as_str());
            if !args.is_empty() {
                let mut array = Array::new();
                for arg in args {
                    array.push(arg.as_str());
                }
                table["args"] = Item::Value(Value::Array(array));
            }
            if !env.is_empty() {
                let mut inline = InlineTable::new();
                let mut pairs: Vec<_> = env.iter().collect();
                pairs.sort_by_key(|(left, _)| left.as_str());
                for (key, value) in pairs {
                    inline.insert(key.as_str(), Value::from(value.as_str()));
                }
                table["env"] = Item::Value(Value::InlineTable(inline));
            }
        }
        McpServerEntry::Http { name, url, auth } => {
            table["transport"] = toml_edit::value("http");
            table["name"] = toml_edit::value(name.as_str());
            table["url"] = toml_edit::value(url.as_str());
            if *auth != McpAuthMode::None {
                let auth = match auth {
                    McpAuthMode::None => unreachable!("filtered above"),
                    McpAuthMode::Bearer => "bearer",
                    McpAuthMode::Oauth => "oauth",
                };
                table["auth"] = toml_edit::value(auth);
            }
        }
    }
    table
}

fn load_document(path: &Path) -> io::Result<DocumentMut> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    contents
        .parse::<DocumentMut>()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn write_document(path: &Path, document: DocumentMut) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, document.to_string())
}

fn mcp_servers_mut(
    document: &mut DocumentMut,
    create_if_missing: bool,
) -> io::Result<Option<&mut ArrayOfTables>> {
    if !document.as_table().contains_key("mcp_servers") {
        if !create_if_missing {
            return Ok(None);
        }
        document["mcp_servers"] = Item::ArrayOfTables(ArrayOfTables::new());
    }

    document["mcp_servers"]
        .as_array_of_tables_mut()
        .map(Some)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "config.toml field `mcp_servers` is not an array of tables",
            )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn add_server_preserves_malformed_rows_and_comments() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let original = r#"
[[mcp_servers]]
name = "good"
transport = "stdio"
command = "/bin/echo"

# keep this broken row exactly as written
[[mcp_servers]]
name = "broken"
transport = "bogus"
url = "https://broken.example/mcp"
"#;
        std::fs::write(&path, original).unwrap();

        add_server(
            &path,
            entry_table(&McpServerEntry::Http {
                name: "added".into(),
                url: "https://example.test/mcp".into(),
                auth: McpAuthMode::Bearer,
            }),
        )
        .unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep this broken row exactly as written"));
        assert!(updated.contains("name = \"broken\"\ntransport = \"bogus\""));
        assert!(updated.contains("name = \"added\""));
        assert!(updated.contains("url = \"https://example.test/mcp\""));
        assert!(updated.contains("auth = \"bearer\""));
    }

    #[test]
    fn remove_server_preserves_malformed_rows_and_comments() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let original = r#"
[[mcp_servers]]
name = "good"
transport = "stdio"
command = "/bin/echo"

# keep this broken row exactly as written
[[mcp_servers]]
name = "broken"
transport = "bogus"
url = "https://broken.example/mcp"
"#;
        std::fs::write(&path, original).unwrap();

        let removed = remove_server(&path, "good").unwrap();
        assert!(removed);

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(!updated.contains("name = \"good\""));
        assert!(updated.contains("# keep this broken row exactly as written"));
        assert!(updated.contains("name = \"broken\"\ntransport = \"bogus\""));
    }

    #[test]
    fn remove_server_missing_name_is_a_no_op() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let original = r#"
[[mcp_servers]]
name = "good"
transport = "stdio"
command = "/bin/echo"
"#;
        std::fs::write(&path, original).unwrap();

        let removed = remove_server(&path, "missing").unwrap();
        assert!(!removed);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn entry_table_writes_stdio_fields_directly() {
        let table = entry_table(&McpServerEntry::Stdio {
            name: "stdio".into(),
            command: "/bin/echo".into(),
            args: vec!["hello".into(), "world".into()],
            env: HashMap::from([("TOKEN".into(), "keyring".into())]),
        });

        assert_eq!(table["transport"].as_str(), Some("stdio"));
        assert_eq!(table["name"].as_str(), Some("stdio"));
        assert_eq!(table["command"].as_str(), Some("/bin/echo"));
        assert_eq!(
            table["args"]
                .as_array()
                .expect("args array")
                .iter()
                .map(|value| value.as_str().expect("string arg"))
                .collect::<Vec<_>>(),
            vec!["hello", "world"]
        );
        assert_eq!(
            table["env"]
                .as_inline_table()
                .and_then(|env| env.get("TOKEN"))
                .and_then(Value::as_str),
            Some("keyring")
        );
    }
}
