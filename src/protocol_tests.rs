use super::*;

#[tokio::test]
async fn framing_round_trips() {
    let (mut a, mut b) = tokio::io::duplex(1024);
    let req = Request::PutFile {
        rel: "a/b.txt".into(),
        size: 3,
        mtime_ms: 7,
        exec: true,
    };
    write_msg(&mut a, &req).await.unwrap();
    write_msg(&mut a, &Response::Ok).await.unwrap();
    assert_eq!(read_msg::<_, Request>(&mut b).await.unwrap(), req);
    assert_eq!(read_msg::<_, Response>(&mut b).await.unwrap(), Response::Ok);
}

#[tokio::test]
async fn limited_read_rejects_over_its_limit() {
    let (mut a, mut b) = tokio::io::duplex(64);
    a.write_all(&(64u32 * 1024 + 1).to_be_bytes())
        .await
        .unwrap();
    let err = read_msg_limited::<_, Request>(&mut b, 64 * 1024)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("64 KiB"), "{err}");
}

#[tokio::test]
async fn oversize_length_is_rejected() {
    let (mut a, mut b) = tokio::io::duplex(64);
    a.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
    let err = read_msg::<_, Request>(&mut b).await.unwrap_err();
    assert!(err.to_string().contains("64 MiB"));
}
