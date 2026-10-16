use agent_client_protocol::Error;

use crate::connection::{Failure, failure};

#[test]
fn an_agent_that_asks_to_sign_in_says_so() {
    assert_eq!(
        failure(Error::auth_required(), &["unrelated".to_owned()]),
        Failure {
            message: Error::auth_required().message,
            sign_in: true,
        }
    );
}

#[test]
fn a_failure_shows_what_the_agent_wrote_last() {
    let error = Error::internal_error().data(serde_json::json!("The agent stopped."));

    assert_eq!(
        failure(
            error,
            &[
                "npm ERR! 404 Not Found".to_owned(),
                "  ".to_owned(),
                "npm ERR! @agentclientprotocol/claude-agent-acp".to_owned(),
            ]
        ),
        Failure {
            message: "The agent stopped.\n\nnpm ERR! 404 Not Found\n\
                      npm ERR! @agentclientprotocol/claude-agent-acp"
                .to_owned(),
            sign_in: false,
        }
    );
}
