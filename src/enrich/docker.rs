//! Container data straight from the Docker socket (D-3). Calling the `docker`
//! command is closed by FR-10, so the API is spoken directly: a plain HTTP/1.1
//! request over a unix socket, which needs no library.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use super::json::{parse, Json};
use crate::model::ContainerInfo;
use crate::util::clean;

const API: &str = "/v1.41";

/// Where the container data comes from, or that it deliberately does not
/// (`--docker-socket none`, the acceptance of FR-3 and D-13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Socket(String),
    Disabled,
}

impl Source {
    pub fn parse(arg: &str) -> Source {
        if arg == "none" {
            Source::Disabled
        } else {
            Source::Socket(arg.to_string())
        }
    }
}

pub struct Docker {
    source: Source,
}

impl Docker {
    pub fn new(source: Source) -> Docker {
        Docker { source }
    }

    pub fn enabled(&self) -> bool {
        matches!(self.source, Source::Socket(_))
    }

    /// The container list keyed by full identifier, the form the cgroup name
    /// carries. `None` means the socket could not be used at all - the rows
    /// then degrade to the short identifier with an unavailability marker.
    pub fn list(&self) -> Option<HashMap<String, ContainerInfo>> {
        let path = match &self.source {
            Source::Socket(p) => p.clone(),
            Source::Disabled => return None,
        };
        let body = request(Path::new(&path), &format!("{API}/containers/json?all=1"))?;
        let value = parse(&body)?;
        let items = value.as_arr()?;
        let mut map = HashMap::new();
        for item in items {
            let id = item.str_of("Id");
            if id.is_empty() {
                continue;
            }
            map.insert(id.clone(), container_info(item));
        }
        Some(map)
    }

    /// The restart count is not in the list, only in the inspection of a single
    /// container, so it is fetched separately and cached by the caller.
    pub fn restart_count(&self, id: &str) -> Option<u64> {
        let path = match &self.source {
            Source::Socket(p) => p.clone(),
            Source::Disabled => return None,
        };
        let body = request(Path::new(&path), &format!("{API}/containers/{id}/json"))?;
        let value = parse(&body)?;
        value
            .get("RestartCount")
            .and_then(|v| v.as_f64())
            .map(|v| v as u64)
    }
}

fn container_info(item: &Json) -> ContainerInfo {
    let name = item
        .get("Names")
        .and_then(|n| n.as_arr())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_default();
    let labels = match item.get("Labels") {
        Some(Json::Obj(items)) => items
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (clean(k), clean(s))))
            .collect(),
        _ => Vec::new(),
    };
    let ports = item
        .get("Ports")
        .and_then(|p| p.as_arr())
        .map(|arr| {
            let mut out: Vec<String> = arr
                .iter()
                .filter_map(|p| {
                    let private = p.get("PrivatePort").and_then(|v| v.as_f64())? as u64;
                    let proto = p.str_of("Type");
                    match p.get("PublicPort").and_then(|v| v.as_f64()) {
                        Some(public) => Some(format!("{}:{private}/{proto}", public as u64)),
                        None => Some(format!("{private}/{proto}")),
                    }
                })
                .collect();
            out.sort();
            out.dedup();
            out
        })
        .unwrap_or_default();
    ContainerInfo {
        name: clean(&name),
        image: clean(&item.str_of("Image")),
        state: clean(&item.str_of("State")),
        status: clean(&item.str_of("Status")),
        created: item
            .get("Created")
            .and_then(|v| v.as_f64())
            .map(crate::enrich::stamp)
            .unwrap_or_default(),
        restarts: None,
        labels,
        ports,
    }
}

/// One request, one connection. The daemon answers a `Connection: close`
/// request with either a length or a chunked body, and both shapes appear in
/// practice depending on the endpoint.
fn request(socket: &Path, url: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1500)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1500)))
        .ok()?;
    let req = format!(
        "GET {url} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head.lines().next()?;
    if !status.contains(" 200") {
        return None;
    }
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        Some(dechunk(body))
    } else {
        Some(body.to_string())
    }
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size_line, tail)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16)
            .unwrap_or(0);
        if size == 0 {
            break;
        }
        if tail.len() < size {
            out.push_str(tail);
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_a_chunked_body() {
        assert_eq!(dechunk("4\r\n[{}]\r\n0\r\n\r\n"), "[{}]");
    }

    #[test]
    fn disabling_the_socket_is_a_source_of_its_own() {
        assert_eq!(Source::parse("none"), Source::Disabled);
        assert!(!Docker::new(Source::Disabled).enabled());
        assert!(Docker::new(Source::parse("/var/run/docker.sock")).enabled());
    }

    #[test]
    fn reads_the_fields_the_rows_and_cards_need() {
        let v = parse(
            r#"{"Id":"a","Names":["/hs-web"],"Image":"nginx:1.25","State":"running",
                "Status":"Up 3 days","Created":0,"Labels":{"role":"web"},
                "Ports":[{"PrivatePort":80,"PublicPort":8080,"Type":"tcp"}]}"#,
        )
        .unwrap();
        let info = container_info(&v);
        assert_eq!(info.name, "hs-web");
        assert_eq!(info.image, "nginx:1.25");
        assert_eq!(info.state, "running");
        assert_eq!(info.ports, vec!["8080:80/tcp".to_string()]);
        assert_eq!(info.labels, vec![("role".to_string(), "web".to_string())]);
    }
}
