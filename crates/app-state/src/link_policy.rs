//! Aviso de segurança dos links: a escolha lembrada vale por site (esquema + host), para todos
//! os PDFs, como "Lembrar desta ação para este site" no Acrobat.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkDecision {
    Allow,
    Block,
}

/// Teto de sites lembrados (o arquivo de preferências não cresce sem limite).
pub const MAX_SITES: usize = 500;

/// Chave do site: `esquema://host` em minúsculas, sem usuário nem porta. Endereços sem host
/// (`mailto:`) são lembrados pelo esquema. `None` se não der para entender o endereço.
pub fn site_key(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return None;
    }
    let Some(rest) = rest.strip_prefix("//") else {
        return Some(format!("{scheme}:"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        format!("[{}]", v6.split(']').next()?)
    } else {
        host_port.split(':').next().unwrap_or("").to_string()
    };
    (!host.is_empty()).then(|| format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// Escolha lembrada para o endereço, se houver.
pub fn remembered(sites: &BTreeMap<String, LinkDecision>, url: &str) -> Option<LinkDecision> {
    sites.get(&site_key(url)?).copied()
}

/// Lembra a escolha para o site do endereço (respeitando o teto).
pub fn remember(sites: &mut BTreeMap<String, LinkDecision>, url: &str, d: LinkDecision) {
    if let Some(key) = site_key(url) {
        if sites.len() < MAX_SITES || sites.contains_key(&key) {
            sites.insert(key, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_key_normalizes_host() {
        let k = |u| site_key(u);
        assert_eq!(
            k("HTTPS://Ex.com:8080/a?b#c").as_deref(),
            Some("https://ex.com")
        );
        assert_eq!(
            k("https://user:senha@ex.com/x").as_deref(),
            Some("https://ex.com")
        );
        assert_eq!(k("http://[::1]:80/").as_deref(), Some("http://[::1]"));
        assert_eq!(k("mailto:x@y.com").as_deref(), Some("mailto:"));
        assert_eq!(
            k("https://evil.com@ex.com").as_deref(),
            Some("https://ex.com")
        );
        assert_eq!(k("sem esquema"), None);
        assert_eq!(k("https://"), None);
    }

    #[test]
    fn remembered_choice_matches_by_site() {
        let mut sites = BTreeMap::new();
        remember(&mut sites, "https://ex.com/a", LinkDecision::Block);
        assert_eq!(
            remembered(&sites, "https://EX.com:443/b"),
            Some(LinkDecision::Block)
        );
        assert_eq!(remembered(&sites, "http://ex.com/b"), None, "outro esquema");
        assert_eq!(remembered(&sites, "https://outro.com"), None);
    }

    #[test]
    fn remembered_sites_are_capped() {
        let mut sites = BTreeMap::new();
        for i in 0..MAX_SITES + 10 {
            remember(
                &mut sites,
                &format!("https://s{i}.com"),
                LinkDecision::Allow,
            );
        }
        assert_eq!(sites.len(), MAX_SITES);
    }
}
