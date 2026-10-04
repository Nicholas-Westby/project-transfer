use super::*;
use rustls::client::ResolvesClientCert;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn identity() -> (tempfile::TempDir, Identity) {
    let dir = tempfile::tempdir().unwrap();
    let id = Identity::load_or_create(dir.path()).unwrap();
    (dir, id)
}

/// Presents `cert`'s certificate but signs with `key`'s key: what someone who
/// copied a paired computer's certificate, but not its key, would do.
fn mismatched(cert: &Identity, key: &Identity) -> Arc<CertifiedKey> {
    let signing = provider()
        .key_provider
        .load_private_key(cert_and_key(key).1)
        .unwrap();
    Arc::new(CertifiedKey::new(cert_and_key(cert).0, signing))
}

#[derive(Debug)]
struct Fixed(Arc<CertifiedKey>);

impl ResolvesClientCert for Fixed {
    fn resolve(&self, _: &[&[u8]], _: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
    fn has_certs(&self) -> bool {
        true
    }
}

impl ResolvesServerCert for Fixed {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

/// Runs a handshake over an in-memory pipe; Ok carries the fingerprints each
/// side saw for the other.
async fn handshake(server: ServerConfig, client: ClientConfig) -> Result<(String, String), String> {
    let (c, s) = tokio::io::duplex(64 * 1024);
    let acceptor = TlsAcceptor::from(Arc::new(server));
    let connector = TlsConnector::from(Arc::new(client));
    let name = ServerName::try_from(SERVER_NAME).unwrap();
    let (srv, cli) = tokio::join!(acceptor.accept(s), connector.connect(name, c));
    let (srv, cli) = match (srv, cli) {
        (Ok(s), Ok(c)) => (s, c),
        (s, c) => return Err(format!("server {:?} client {:?}", s.err(), c.err())),
    };
    let mut srv = srv;
    let mut cli = cli;
    // TLS 1.3 client auth is only checked once the server reads; force it.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    cli.write_all(b"x").await.map_err(|e| e.to_string())?;
    cli.flush().await.map_err(|e| e.to_string())?;
    let mut b = [0u8; 1];
    srv.read_exact(&mut b).await.map_err(|e| e.to_string())?;
    let seen_by_server = peer_fingerprint(srv.get_ref().1.peer_certificates()).unwrap();
    let seen_by_client = peer_fingerprint(cli.get_ref().1.peer_certificates()).unwrap();
    Ok((seen_by_server, seen_by_client))
}

#[tokio::test]
async fn any_self_signed_pair_completes_the_handshake() {
    let ((_d1, a), (_d2, b)) = (identity(), identity());
    let (srv_saw, cli_saw) = handshake(server_config(&b).unwrap(), client_config(&a).unwrap())
        .await
        .unwrap();
    assert_eq!(srv_saw, a.fingerprint());
    assert_eq!(cli_saw, b.fingerprint());
}

#[tokio::test]
async fn client_without_the_certificates_key_is_rejected() {
    let ((_d1, victim), (_d2, thief), (_d3, b)) = (identity(), identity(), identity());
    let provider = provider();
    let client = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert {
            algs: provider.signature_verification_algorithms,
        }))
        .with_client_cert_resolver(Arc::new(Fixed(mismatched(&victim, &thief))));
    assert!(handshake(server_config(&b).unwrap(), client).await.is_err());
}

#[tokio::test]
async fn server_without_the_certificates_key_is_rejected() {
    let ((_d1, victim), (_d2, thief), (_d3, a)) = (identity(), identity(), identity());
    let provider = provider();
    let server = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_client_cert_verifier(Arc::new(AnyCert {
            algs: provider.signature_verification_algorithms,
        }))
        .with_cert_resolver(Arc::new(Fixed(mismatched(&victim, &thief))));
    assert!(handshake(server, client_config(&a).unwrap()).await.is_err());
}
