//! Enviar formulário (a ação do Acrobat): POST para http(s) numa thread, com tempo-limite e
//! resposta curta, ou o programa de e-mail com os dados no corpo (mailto não leva anexo).
use std::io::Read;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

/// Tempo-limite do envio e maior resposta lida.
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE: u64 = 1024 * 1024;

/// Envio em andamento: o resultado chega pelo canal.
pub struct Sending {
    rx: Receiver<Result<u16, String>>,
}

impl Sending {
    /// O resultado, se já chegou: o código HTTP ou o erro.
    pub fn poll(&self) -> Option<Result<u16, String>> {
        self.rx.try_recv().ok()
    }
}

/// Manda `body` por POST para `url` (já conferido: http ou https).
pub fn post(url: String, body: Vec<u8>, content_type: String) -> Sending {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(TIMEOUT)
            .redirects(0)
            .resolver(public_only)
            .build();
        let result = match agent
            .post(&url)
            .set("Content-Type", &content_type)
            .send_bytes(&body)
        {
            // redirecionamento não é seguido (o destino podia ser interno): conta como falha
            Ok(resp) if (300..400).contains(&resp.status()) => Err(format!(
                "o servidor redirecionou ({}) e o envio não segue redirecionamentos",
                resp.status()
            )),
            Ok(resp) => {
                // a resposta não é mostrada: só lida (com teto) para fechar a conexão direito
                let status = resp.status();
                let mut sink = Vec::new();
                let _ = resp.into_reader().take(MAX_RESPONSE).read_to_end(&mut sink);
                Ok(status)
            }
            Err(ureq::Error::Status(code, _)) => Err(format!("o servidor respondeu {code}")),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(result);
    });
    Sending { rx }
}

/// Resolve o host e fica só com os IPs públicos: o nome já conferido não pode apontar, na hora do
/// envio, para a própria máquina ou a rede interna (DNS rebinding).
fn public_only(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = netloc
        .to_socket_addrs()?
        .filter(|a| pdf_core::is_public_ip(a.ip()))
        .collect();
    if addrs.is_empty() {
        return Err(std::io::Error::other("endereço interno recusado"));
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_refuses_internal_addresses() {
        assert!(public_only("127.0.0.1:80").is_err());
        assert!(public_only("[::1]:443").is_err());
        assert!(public_only("8.8.8.8:80").is_ok());
    }
}
