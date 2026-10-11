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
    // espaço, controle ou barra invertida: os navegadores corrigem o endereço e podem ir a outro
    // host; um endereço assim nunca é lembrado (o aviso aparece sempre)
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return None;
    }
    let (scheme, rest) = url.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !scheme_ok {
        return None;
    }
    let Some(rest) = rest.strip_prefix("//") else {
        // só um esquema sem host (mailto:) é lembrado pelo esquema; "https:host" sem "//" ainda
        // vai a um host no navegador, e lembrar "https:" liberaria todos
        return (scheme == "mailto").then(|| format!("{scheme}:"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (h, after) = v6.split_once(']')?;
            let ok = !h.is_empty()
                && h.chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
            let port = match after {
                "" => None,
                a => Some(a.strip_prefix(':')?),
            };
            (ok.then(|| format!("[{h}]"))?, port)
        }
        None => match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        },
    };
    let host_ok = host.starts_with('[')
        || (!host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
    let port_ok =
        port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()));
    (host_ok && port_ok).then(|| format!("{scheme}://{}", host.to_ascii_lowercase()))
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
        assert_eq!(k("http://[::1]:80/").as_deref(), Some("http://[::1]"));
        assert_eq!(k("mailto:x@y.com").as_deref(), Some("mailto:"));
        assert_eq!(k("sem esquema"), None);
        assert_eq!(k("https://"), None);
    }

    #[test]
    fn ambiguous_urls_are_never_remembered() {
        // onde o navegador e um analisador simples podem discordar do host: sempre perguntar
        for url in [
            "https://user:senha@ex.com/x",
            "https://evil.com@ex.com",
            "https://ex.com\\@evil.com/",
            "https://ex.com\\.evil.com/",
            "https://ex.com\t/",
            "https://ex.com\n.evil.com",
            "https://ex%2ecom/",
            "https://ex.com:80:90/",
            "https://ex.com:/",
            "https://exé.com/",
            " https://ex.com",
            // sem "//" o navegador ainda acha o host (WHATWG): nunca lembrar pelo esquema
            "https:evil.com",
            "http:/evil.com",
            "HTTPS:ex.com",
            "ftp:host/x",
        ] {
            assert_eq!(site_key(url), None, "{url:?}");
        }
        let mut sites = BTreeMap::new();
        remember(&mut sites, "https://ex.com", LinkDecision::Allow);
        assert_eq!(remembered(&sites, "https://ex.com\\@evil.com"), None);
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
