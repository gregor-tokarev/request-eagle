use super::ResponseView;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

impl ResponseView {
    pub(crate) fn automation_snapshot(&self, offset: usize, limit: usize) -> Result<Value, String> {
        let scripts: Vec<_> = self
            .scripts
            .iter()
            .map(|report| {
                let tests: Vec<_> = report
                    .tests
                    .iter()
                    .map(|test| {
                        json!({
                            "name": test.name,
                            "error": test.error,
                        })
                    })
                    .collect();
                let logs: Vec<_> = report
                    .logs
                    .iter()
                    .map(|log| {
                        json!({
                            "level": log.level,
                            "message": log.message,
                        })
                    })
                    .collect();

                json!({
                    "phase": report.phase.label(),
                    "error": report.error,
                    "tests": tests,
                    "logs": logs,
                })
            })
            .collect();

        let Some(content) = &self.content else {
            return Ok(json!({
                "loading": self.loading,
                "failed": self.error,
                "message": self.message.as_ref(),
                "scripts": scripts,
            }));
        };

        let response = content.http();
        if offset > response.body.len() {
            return Err("Body offset is beyond the response".into());
        }

        let end = offset.saturating_add(limit).min(response.body.len());
        let headers: Vec<_> = response
            .headers
            .iter()
            .map(|(name, value)| {
                json!({
                    "name": name.as_str(),
                    "value": String::from_utf8_lossy(value.as_bytes()),
                    "value_base64": STANDARD.encode(value.as_bytes()),
                })
            })
            .collect();
        let cookies: Vec<_> = headers
            .iter()
            .filter(|header| header["name"] == "set-cookie")
            .collect();
        let metrics = response.metrics;

        Ok(json!({
            "loading": false,
            "status": response.status.as_u16(),
            "version": format!("{:?}", response.version),
            "headers": headers,
            "cookies": cookies,
            "body": {
                "encoding": "base64",
                "data": STANDARD.encode(&response.body[offset..end]),
                "offset": offset,
                "next_offset": (end < response.body.len()).then_some(end),
                "total_bytes": response.body.len(),
            },
            "timing_ms": {
                "elapsed": content.execution.elapsed.as_secs_f64() * 1000.,
                "prepare": metrics.prepare.as_secs_f64() * 1000.,
                "waiting": metrics.waiting.as_secs_f64() * 1000.,
                "download": metrics.download.as_secs_f64() * 1000.,
                "formatting": content.processing.as_secs_f64() * 1000.,
            },
            "size": {
                "request_headers": metrics.request_header_bytes,
                "response_headers": metrics.response_header_bytes,
                "request_body": metrics.request_body_bytes,
                "encoded_response_body": metrics.encoded_response_body_bytes,
            },
            "scripts": scripts,
        }))
    }
}
