use gpui_kit::{App, Global};

/// Session approval is deliberately not persisted to preferences or disk.
#[derive(Default)]
pub struct CliAccess {
    pub token: Option<String>,
    pub error: Option<String>,
}

impl Global for CliAccess {}

impl CliAccess {
    pub fn init(cx: &mut App) {
        if !cx.has_global::<Self>() {
            let mut access = Self::default();
            match std::env::var("REQUEST_EAGLE_AUTOMATION_TOKEN") {
                Ok(token)
                    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                {
                    access.token = Some(token);
                }
                Err(std::env::VarError::NotPresent) => {}
                _ => access.error = Some(
                    "REQUEST_EAGLE_AUTOMATION_TOKEN must contain 64 random hexadecimal characters"
                        .into(),
                ),
            }
            cx.set_global(access);
        }
    }

    pub fn enable(cx: &mut App) {
        let mut bytes = [0u8; 32];
        let access = match getrandom::fill(&mut bytes) {
            Ok(()) => Self {
                token: Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect()),
                error: None,
            },
            Err(error) => Self {
                token: None,
                error: Some(format!("Cannot create a CLI session: {error}")),
            },
        };
        cx.set_global(access);
    }

    pub fn disable(cx: &mut App) {
        cx.set_global(Self::default());
    }

    pub fn authorizes(&self, token: &str) -> bool {
        self.token
            .as_deref()
            .is_some_and(|expected| expected == token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn session_approval_generates_and_revokes_credentials(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(CliAccess::default());
            assert!(!cx.global::<CliAccess>().authorizes(""));
            CliAccess::enable(cx);
            let first = cx.global::<CliAccess>().token.clone().unwrap();
            assert_eq!(first.len(), 64);
            assert!(cx.global::<CliAccess>().authorizes(&first));
            assert!(!cx.global::<CliAccess>().authorizes("wrong"));
            CliAccess::disable(cx);
            assert!(!cx.global::<CliAccess>().authorizes(&first));
            CliAccess::enable(cx);
            assert!(!cx.global::<CliAccess>().authorizes(&first));
        });
    }
}
