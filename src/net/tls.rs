//! TLS configs that accept any certificate during the handshake. Trust comes
//! afterwards, from comparing the peer's fingerprint with the stored one, but
//! the handshake signature is still checked so the peer must hold the key of
//! the certificate it presents.

use crate::identity::{Identity, fingerprint_of};
use anyhow::Context;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
};
use std::sync::Arc;

/// Certificates are self-signed, so the name only has to be well formed.
pub const SERVER_NAME: &str = "project-transfer";

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn cert_and_key(id: &Identity) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    (
        vec![CertificateDer::from(id.cert_der.clone())],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_der.clone())),
    )
}

pub fn server_config(id: &Identity) -> anyhow::Result<ServerConfig> {
    let provider = provider();
    let verifier = AnyCert {
        algs: provider.signature_verification_algorithms,
    };
    let (certs, key) = cert_and_key(id);
    ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(verifier))
        .with_single_cert(certs, key)
        .context("could not load this computer's TLS identity")
}

pub fn client_config(id: &Identity) -> anyhow::Result<ClientConfig> {
    let provider = provider();
    let verifier = AnyCert {
        algs: provider.signature_verification_algorithms,
    };
    let (certs, key) = cert_and_key(id);
    ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_client_auth_cert(certs, key)
        .context("could not load this computer's TLS identity")
}

/// The fingerprint of the certificate the peer proved it holds the key for.
pub fn peer_fingerprint(certs: Option<&[CertificateDer<'_>]>) -> anyhow::Result<String> {
    let cert = certs
        .and_then(|c| c.first())
        .context("the other computer did not present a certificate")?;
    Ok(fingerprint_of(cert.as_ref()))
}

#[derive(Debug)]
struct AnyCert {
    algs: WebPkiSupportedAlgorithms,
}

impl AnyCert {
    fn tls12(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn tls13(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }
}

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.tls12(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

impl ClientCertVerifier for AnyCert {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.tls12(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

#[cfg(test)]
#[path = "tls_tests.rs"]
mod tests;
