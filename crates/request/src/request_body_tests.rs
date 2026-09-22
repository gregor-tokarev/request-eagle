use std::time::Duration;

use crate::{FormBody, MultipartField};

#[test]
fn multipart_text_encoding_yields_to_the_request_deadline() {
    smol::block_on(async {
        // Text-only forms do not await file reads. Their buffer assembly and
        // boundary scan must still let the executor poll the timeout future.
        let form = FormBody::Multipart(vec![MultipartField::Text {
            name: "large text".into(),
            value: "x".repeat(16 * 1024 * 1024),
        }]);
        let mut encoding = Box::pin(form.encode());
        let deadline = smol::Timer::after(Duration::from_millis(1));
        let timed_out = smol::future::or(
            async {
                encoding.as_mut().await.unwrap();
                false
            },
            async {
                deadline.await;
                true
            },
        )
        .await;

        assert!(timed_out, "form encoding blocked the deadline future");

        // Finish the worker before this test exits, including its allocations.
        let (body, content_type) = encoding.await.unwrap();
        assert!(body.len() > 16 * 1024 * 1024);
        assert!(content_type.starts_with("multipart/form-data; boundary="));
    });
}
