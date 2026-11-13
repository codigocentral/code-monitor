use super::*;
use crate::unit_tls::{self as sdk, State};
use std::sync::{Arc, Mutex};
fn config() -> TlsConfig {
    TlsConfig {
        ca_cert_path: String::new(),
        client_cert_path: None,
        client_key_path: None,
        danger_skip_verify: true,
    }
}
fn connector(state: sdk::SharedState) -> SkipVerifyConnector {
    sdk::next(state);
    configure_tls(Endpoint::from_static("https://unit.invalid:443"), &config())
        .unwrap()
        .connector
        .unwrap()
}
#[tokio::test]
async fn custom_connector_preserves_http_tls_and_invalid_domain_failures() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut connector = connector(state.clone());
    let mut cx = TaskContext::from_waker(std::task::Waker::noop());
    assert!(matches!(connector.poll_ready(&mut cx), Poll::Ready(Ok(()))));
    state.lock().unwrap().ready_error = true;
    assert!(matches!(connector.poll_ready(&mut cx), Poll::Ready(Err(_))));
    state.lock().unwrap().http_error = true;
    let uri = "https://unit.invalid:443/metrics"
        .parse::<http::Uri>()
        .unwrap();
    assert!(connector
        .call(uri.clone())
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("unit TLS transport failure"));
    state.lock().unwrap().http_error = false;
    connector.domain = "\0".into();
    assert_eq!(
        connector.call(uri.clone()).await.err().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );
    connector.domain = "unit.invalid".into();
    state.lock().unwrap().tls_error = true;
    assert!(connector
        .call(uri.clone())
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("unit TLS transport failure"));
    state.lock().unwrap().tls_error = false;
    let _stream = connector.call(uri.clone()).await.unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.uri.as_ref(), Some(&uri));
    assert_eq!(state.enforce_http, Some(false));
    assert_eq!(state.alpn, vec![b"h2".to_vec()]);
    assert!(state.domain.as_ref().unwrap().contains("unit.invalid"));
}
#[test]
fn stream_delegates_reads_writes_flush_shutdown_and_connection_metadata() {
    let state = Arc::new(Mutex::new(State {
        reads: std::collections::VecDeque::from(b"reply".to_vec()),
        ..Default::default()
    }));
    let mut stream = SkipVerifyStream(sdk::TlsStream(state.clone()));
    let mut cx = TaskContext::from_waker(std::task::Waker::noop());
    let mut bytes = [0; 5];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    assert!(matches!(
        Pin::new(&mut stream).poll_read(&mut cx, &mut buffer),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(buffer.filled(), b"reply");
    assert!(matches!(
        Pin::new(&mut stream).poll_write(&mut cx, b"request"),
        Poll::Ready(Ok(7))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_flush(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_shutdown(&mut cx),
        Poll::Ready(Ok(()))
    ));
    let _metadata = stream.connected();
    {
        let state = state.lock().unwrap();
        assert_eq!(state.writes, b"request");
        assert_eq!(state.flushes, 1);
        assert_eq!(state.shutdowns, 1);
    }
    state.lock().unwrap().stream_error = true;
    assert!(matches!(
        Pin::new(&mut stream).poll_read(&mut cx, &mut buffer),
        Poll::Ready(Err(_))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_write(&mut cx, b"request"),
        Poll::Ready(Err(_))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_flush(&mut cx),
        Poll::Ready(Err(_))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_shutdown(&mut cx),
        Poll::Ready(Err(_))
    ));
}
#[test]
fn certificate_and_key_read_failures_keep_their_path_context() {
    let temp = tempfile::tempdir().unwrap();
    let ca = temp.path().join("ca.pem");
    let cert = temp.path().join("client.pem");
    let key = temp.path().join("key.pem");
    std::fs::write(&ca, "unit CA").unwrap();
    std::fs::write(&cert, "unit client certificate").unwrap();
    let mut tls = TlsConfig {
        ca_cert_path: ca.display().to_string(),
        client_cert_path: Some(cert.display().to_string()),
        client_key_path: Some(key.display().to_string()),
        danger_skip_verify: false,
    };
    let error = configure_tls(Endpoint::from_static("https://unit.invalid"), &tls)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Failed to read client key"));
    assert!(error.contains("key.pem"));
    std::fs::write(&key, "unit invalid key").unwrap();
    let error = configure_tls(Endpoint::from_static("https://unit.invalid"), &tls)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Failed to apply TLS configuration"));
    tls.client_cert_path = None;
    tls.client_key_path = None;
    // A CA without PEM entries is accepted at configuration time by the current
    // tonic provider; preserve this behavior and avoid any handshake.
    assert!(configure_tls(Endpoint::from_static("https://unit.invalid"), &tls).is_ok());
}
