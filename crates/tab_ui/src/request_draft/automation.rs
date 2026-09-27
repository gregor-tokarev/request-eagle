use crate::RequestDraft;
use gpui_kit::{App, Context, Window};
use serde_json::{Value, json};

impl RequestDraft {
    /// Invalidate controls that retain their own editable state. Preserve the
    /// saved baseline, response, environment and in-flight request snapshot.
    pub fn replace_request(
        &mut self,
        request: collection::HttpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.request.path != request.path {
            self.url = None;
            self.url_completion = None;
            self.url_subscription = None;
        }

        if self.request.query != request.query {
            self.params = None;
            self.params_subscription = None;
        }

        if self.request.headers != request.headers {
            self.headers = None;
            self.headers_subscription = None;
        }

        if self.request.body != request.body {
            self.body = None;
            self.body_task = None;
            self.body_completion = None;
            self.body_subscription = None;
            self.body_json_valid = false;
        }

        for (index, changed) in [
            self.request.scripts.pre_request != request.scripts.pre_request,
            self.request.scripts.post_response != request.scripts.post_response,
        ]
        .into_iter()
        .enumerate()
        {
            if changed {
                self.script_editors[index] = None;
                self.script_subscriptions[index] = None;
            }
        }

        self.request = request;
        self.set_method(self.request.method, cx);
        self.prepare(window, cx);
        cx.notify();
    }

    pub fn send_from_automation(
        &mut self,
        trust_scripts: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.is_sending() {
            return Err("A request is already running in this tab".into());
        }
        if self.script_trust_prompt_open {
            return Err("Resolve the open script trust dialog first".into());
        }
        if !self.request.scripts.is_empty()
            && self.trusted_scripts.as_ref() != Some(&self.request.scripts)
        {
            if !trust_scripts {
                return Err(
                    "Review the draft scripts, then set trust_scripts=true to approve them".into(),
                );
            }
            self.trusted_scripts = Some(self.request.scripts.clone());
        }
        self.send(window, cx);
        Ok(())
    }

    pub fn automation_response(
        &self,
        offset: usize,
        limit: usize,
        cx: &App,
    ) -> Result<Value, String> {
        match &self.response {
            Some(response) => response.read(cx).automation_snapshot(offset, limit),
            None => Ok(json!({"loading": false, "state": "empty"})),
        }
    }
}
