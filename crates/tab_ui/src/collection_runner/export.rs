use std::path::PathBuf;

use gpui_kit::*;
use serde_json::{Value, json};

use super::run::Outcome;
use super::runner::{CollectionRunner, Run};

/// A run's results as JSON: its summary, then each request it sent with its
/// outcome and tests.
fn report(name: &str, run: &Run) -> Value {
    let results = run
        .results
        .iter()
        .map(|result| {
            let request = &run.requests[result.position.index];
            let (status, time, skipped, error) = match &result.outcome {
                Outcome::Response {
                    status, elapsed, ..
                } => (Some(status.as_u16()), Some(elapsed.as_millis()), None, None),
                Outcome::Skipped(reason) => (None, None, Some(reason), None),
                Outcome::Failed(message) => (None, None, None, Some(message)),
            };

            json!({
                "iteration": result.position.iteration + 1,
                "id": request.id,
                "name": request.name,
                "folders": request.folders,
                "method": request.request.method.as_str(),
                "url": result.url.as_deref().unwrap_or(&request.request.path),
                "status": status,
                "time": time,
                "skipped": skipped,
                "error": error,
                "scriptErrors": result
                    .scripts
                    .iter()
                    .filter_map(|report| {
                        report
                            .error
                            .as_ref()
                            .map(|error| format!("{}: {error}", report.label()))
                    })
                    .collect::<Vec<_>>(),
                "tests": result
                    .tests()
                    .map(|test| json!({
                        "name": test.name,
                        "passed": test.error.is_none(),
                        "error": test.error,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let (passed, failed) = run.results.iter().flat_map(|result| result.tests()).fold(
        (0, 0),
        |(passed, failed), test| match test.error {
            None => (passed + 1, failed),
            Some(_) => (passed, failed + 1),
        },
    );

    json!({
        "name": name,
        "source": "Runner",
        "environment": run.environment,
        "startedAt": run.started_at.to_rfc3339(),
        "iterations": run.iterations,
        "duration": run.duration().as_millis(),
        "totalPass": passed,
        "totalFail": failed,
        "results": results,
    })
}

impl CollectionRunner {
    /// Save the run's results to a JSON file the user chooses.
    pub(super) fn export_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = &self.run else {
            return;
        };

        let report = report(&self.name, run);
        let directory = std::env::home_dir()
            .map(|home| home.join("Downloads"))
            .filter(|downloads| downloads.is_dir())
            .or_else(std::env::home_dir)
            .unwrap_or_default();
        let file_name = format!("{} run.json", self.name);
        let path = cx.prompt_for_new_path(&directory, Some(&file_name));

        cx.spawn_in(window, async move |this, cx| {
            let saved: Result<PathBuf, SharedString> = match path.await {
                Ok(Ok(Some(path))) => cx
                    .background_spawn(async move {
                        let json = serde_json::to_vec_pretty(&report)?;
                        std::fs::write(&path, json)?;
                        Ok::<_, std::io::Error>(path)
                    })
                    .await
                    .map_err(|error| format!("Could not export the results: {error}").into()),
                Ok(Err(error)) => Err(format!("Could not open the save dialog: {error}").into()),
                // The dialog was cancelled.
                _ => return,
            };

            let _ = this.update(cx, |this, cx| {
                this.exported = Some(saved);
                cx.notify();
            });
        })
        .detach();
    }
}
