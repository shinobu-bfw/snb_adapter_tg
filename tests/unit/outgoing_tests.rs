use super::*;

#[test]
fn outgoing_text_keeps_single_format() {
    let mut text = OutgoingText::default();
    text.push_text("hello".to_string(), Some(TextFormat::Html));
    text.push_text(" world".to_string(), Some(TextFormat::Html));

    let payload = text.into_payload().unwrap();

    assert_eq!(payload.text, "hello world");
    assert_eq!(payload.format, Some(TextFormat::Html));
}

#[test]
fn outgoing_text_drops_mixed_formats() {
    let mut text = OutgoingText::default();
    text.push_text("hello".to_string(), Some(TextFormat::Markdown));
    text.push_text(" world".to_string(), None);

    let payload = text.into_payload().unwrap();

    assert_eq!(payload.text, "hello world");
    assert_eq!(payload.format, None);
}

#[test]
fn merge_captions_combines_distinct_text() {
    let merged = merge_captions(
        Some(FormattedPayload {
            text: "first".to_string(),
            format: Some(TextFormat::Html),
        }),
        Some("second".to_string()),
    )
    .unwrap();

    assert_eq!(merged.text, "first\nsecond");
    assert_eq!(merged.format, None);
}

#[test]
fn decode_base64_image_accepts_data_url() {
    let decoded = decode_base64_image("data:image/png;base64,aGVsbG8=").unwrap();

    assert_eq!(decoded, b"hello");
}
