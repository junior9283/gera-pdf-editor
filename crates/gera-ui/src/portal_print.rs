//! Linux: a janela de impressão do sistema pelo portal (`org.freedesktop.portal.Print`, a do KDE
//! e do GNOME). `prepare` mostra a janela e devolve o token das opções escolhidas; `print` manda
//! o PDF com esse token, e o sistema imprime.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.Print";

/// O que a janela respondeu.
#[derive(Debug, PartialEq)]
pub enum Prepared {
    /// Imprimir, com o token das opções escolhidas.
    Token(u32),
    Cancelled,
}

/// A sessão do usuário (sem D-Bus: erro, e o app usa o diálogo próprio).
pub fn session() -> Result<Connection, String> {
    Connection::session().map_err(|e| e.to_string())
}

/// Chama `method` do portal e espera a resposta no objeto Request (que o portal cria com o
/// `handle_token`): o código (0 = ok, 1 = cancelado) e os resultados.
fn call(
    conn: &Connection,
    method: &str,
    body: impl Fn(HashMap<&str, Value<'_>>) -> Result<zbus::message::Message, zbus::Error>,
) -> Result<(u32, HashMap<String, OwnedValue>), String> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let handle = format!(
        "gera{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let sender = conn
        .unique_name()
        .ok_or("sem nome no barramento")?
        .trim_start_matches(':')
        .replace('.', "_");
    let request = format!("{PATH}/request/{sender}/{handle}");
    // assina a resposta antes de chamar (ela pode chegar logo)
    let req = Proxy::new(
        conn,
        DEST,
        request.as_str(),
        "org.freedesktop.portal.Request",
    )
    .map_err(|e| e.to_string())?;
    let mut responses = req.receive_signal("Response").map_err(|e| e.to_string())?;
    let mut options = HashMap::new();
    options.insert("handle_token", Value::from(handle.clone()));
    options.insert("modal", Value::from(true));
    body(options).map_err(|e| format!("{method}: {e}"))?;
    let msg = responses.next().ok_or("o portal não respondeu")?;
    msg.body()
        .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        .map_err(|e| e.to_string())
}

/// Mostra a janela de impressão do sistema; `ranges` ("1-3,7") já escolhe as páginas.
pub fn prepare(conn: &Connection, title: &str, ranges: Option<&str>) -> Result<Prepared, String> {
    let proxy = Proxy::new(conn, DEST, PATH, IFACE).map_err(|e| e.to_string())?;
    let mut settings: HashMap<&str, Value<'_>> = HashMap::new();
    if let Some(r) = ranges {
        settings.insert("print-pages", Value::from("ranges"));
        settings.insert("page-ranges", Value::from(r));
    }
    let empty: HashMap<&str, Value<'_>> = HashMap::new();
    let (code, results) = call(conn, "PreparePrint", |options| {
        proxy.call_method("PreparePrint", &("", title, &settings, &empty, options))
    })?;
    match code {
        0 => {
            let token = results
                .get("token")
                .and_then(|v| u32::try_from(v).ok())
                .ok_or("o portal não devolveu o token")?;
            Ok(Prepared::Token(token))
        }
        1 => Ok(Prepared::Cancelled),
        n => Err(format!("o portal recusou ({n})")),
    }
}

/// Manda o PDF `file` para impressão com as opções do `token`.
pub fn print(
    conn: &Connection,
    title: &str,
    file: &std::path::Path,
    token: u32,
) -> Result<(), String> {
    let proxy = Proxy::new(conn, DEST, PATH, IFACE).map_err(|e| e.to_string())?;
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let fd = zbus::zvariant::Fd::from(&f);
    let (code, _) = call(conn, "Print", |mut options| {
        options.insert("token", Value::from(token));
        proxy.call_method("Print", &("", title, &fd, options))
    })?;
    match code {
        0 => Ok(()),
        1 => Err("impressão cancelada".into()),
        n => Err(format!("o portal recusou ({n})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::sync::{Arc, Mutex};

    /// Portal falso: PreparePrint responde com o token 7 (ou cancela o título "cancelar");
    /// Print guarda o conteúdo do arquivo recebido.
    /// O que o portal falso recebeu: (token, conteúdo do arquivo).
    type Printed = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

    struct FakePortal {
        printed: Printed,
    }

    fn handle_path(
        header: &zbus::message::Header<'_>,
        options: &HashMap<String, OwnedValue>,
    ) -> String {
        let sender = header
            .sender()
            .unwrap()
            .trim_start_matches(':')
            .replace('.', "_");
        let token: String = options
            .get("handle_token")
            .unwrap()
            .clone()
            .try_into()
            .unwrap();
        format!("{PATH}/request/{sender}/{token}")
    }

    #[zbus::interface(name = "org.freedesktop.portal.Print")]
    impl FakePortal {
        // a assinatura é a do método D-Bus do portal
        #[allow(clippy::too_many_arguments)]
        async fn prepare_print(
            &self,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
            _parent: String,
            title: String,
            settings: HashMap<String, OwnedValue>,
            _page_setup: HashMap<String, OwnedValue>,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::zvariant::OwnedObjectPath {
            let path = handle_path(&header, &options);
            let mut results: HashMap<&str, Value<'_>> = HashMap::new();
            results.insert("token", Value::from(7u32));
            let code = if title == "cancelar" { 1u32 } else { 0 };
            if code == 0 {
                // as páginas pedidas chegam como configuração da janela
                let r: String = settings
                    .get("page-ranges")
                    .unwrap()
                    .clone()
                    .try_into()
                    .unwrap();
                assert_eq!(r, "1-3");
            }
            conn.emit_signal(
                header.sender().cloned(),
                path.as_str(),
                "org.freedesktop.portal.Request",
                "Response",
                &(code, results),
            )
            .await
            .unwrap();
            zbus::zvariant::ObjectPath::try_from(path).unwrap().into()
        }

        async fn print(
            &self,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
            _parent: String,
            _title: String,
            fd: zbus::zvariant::OwnedFd,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::zvariant::OwnedObjectPath {
            let path = handle_path(&header, &options);
            let token: u32 = options.get("token").unwrap().try_into().unwrap();
            let mut bytes = Vec::new();
            std::fs::File::from(std::os::fd::OwnedFd::from(fd))
                .read_to_end(&mut bytes)
                .unwrap();
            self.printed.lock().unwrap().push((token, bytes));
            let results: HashMap<&str, Value<'_>> = HashMap::new();
            conn.emit_signal(
                header.sender().cloned(),
                path.as_str(),
                "org.freedesktop.portal.Request",
                "Response",
                &(0u32, results),
            )
            .await
            .unwrap();
            zbus::zvariant::ObjectPath::try_from(path).unwrap().into()
        }
    }

    #[test]
    fn prints_through_the_portal() {
        // barramento próprio do teste (não toca na sessão do usuário)
        let mut daemon = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut addr = String::new();
        BufReader::new(daemon.stdout.as_mut().unwrap())
            .read_line(&mut addr)
            .unwrap();
        let addr = addr.trim().to_string();
        let printed: Printed = Arc::new(Mutex::new(Vec::new()));
        let _portal = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .name(DEST)
            .unwrap()
            .serve_at(
                PATH,
                FakePortal {
                    printed: printed.clone(),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let conn = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            prepare(&conn, "contrato.pdf", Some("1-3")).unwrap(),
            Prepared::Token(7)
        );
        assert_eq!(
            prepare(&conn, "cancelar", None).unwrap(),
            Prepared::Cancelled
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.pdf");
        std::fs::write(&file, b"%PDF-1.7 teste").unwrap();
        print(&conn, "contrato.pdf", &file, 7).unwrap();
        assert_eq!(*printed.lock().unwrap(), [(7, b"%PDF-1.7 teste".to_vec())]);
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}
