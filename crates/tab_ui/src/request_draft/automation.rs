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
                self.script_signatures[index] = None;
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
        if !self.request.scripts.is_empty() && !trust_scripts {
            return Err(
                "Review the draft scripts, then set trust_scripts=true to approve this send".into(),
            );
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
            None => Ok(json!({"loading": false, "failed": false, "state": "empty"})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{draft::RequestSection, tests::draft};
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn replacing_scripts_rebuilds_intelligence_and_detaches_old_editor(cx: &mut TestAppContext) {
        let (draft, cx) = draft(cx);
        let old_editor = cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                draft.section = RequestSection::Scripts;
                draft.request.scripts.pre_request = "console.log('old');".into();
                let editor = draft.script_state(window, cx);
                let signature = draft.script_signatures[0].as_ref().unwrap().entity_id();

                let mut request = draft.request.clone();
                request.path = "https://example.test/new".into();
                draft.replace_request(request.clone(), window, cx);
                assert_eq!(
                    draft.script_editors[0].as_ref().unwrap().entity_id(),
                    editor.entity_id()
                );
                assert_eq!(
                    draft.script_signatures[0].as_ref().unwrap().entity_id(),
                    signature
                );

                request.scripts.pre_request = "console.log('new');".into();
                draft.replace_request(request, window, cx);
                assert_ne!(
                    draft.script_editors[0].as_ref().unwrap().entity_id(),
                    editor.entity_id()
                );
                assert_ne!(
                    draft.script_signatures[0].as_ref().unwrap().entity_id(),
                    signature
                );
                editor
            })
        });
        cx.update(|window, cx| {
            old_editor.update(cx, |editor, cx| {
                editor.replace_all("stale editor", window, cx)
            })
        });
        cx.run_until_parked();
        assert_eq!(
            draft.read_with(cx, |draft, _| draft.request.scripts.pre_request.clone()),
            "console.log('new');"
        );
    }
}
