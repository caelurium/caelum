use boat_sdk::Error;
use std::fmt::Write;

#[derive(Clone, Copy)]
pub(super) enum Step {
    ReadAccount,
    ListMachines,
    ListEnvironments,
    FindRepository,
    CreateEnvironment,
    ConfigureEnvironment,
    CreateMachine,
    InspectMachine,
    RunCommand,
}

impl Step {
    fn operation(self) -> &'static str {
        match self {
            Self::ReadAccount => "Read Boat account",
            Self::ListMachines => "List VMs",
            Self::ListEnvironments => "List checkout environments",
            Self::FindRepository => "Look up the GitHub repository",
            Self::CreateEnvironment => "Create checkout environment",
            Self::ConfigureEnvironment => "Configure checkout environment",
            Self::CreateMachine => "Create VM",
            Self::InspectMachine => "Inspect VM",
            Self::RunCommand => "Run a command on the VM",
        }
    }

    fn permission_hint(self) -> &'static str {
        match self {
            Self::ReadAccount => "Check that the key has account.read.",
            Self::ListMachines | Self::InspectMachine => "Check that the key has sandbox.read for this VM or environment.",
            Self::ListEnvironments => "Check that the key has environment.read for the checkout environment.",
            Self::FindRepository => "Check the key’s access to Boat’s repository list and the repository’s GitHub connection in Boat.",
            Self::CreateEnvironment | Self::ConfigureEnvironment => "Automatic setup needs environment.write. Check that permission on the connected key in Boat → API Keys.",
            Self::CreateMachine => "Check that the key has sandbox.create for the checkout environment.",
            Self::RunCommand => "Check that the key has exec for this VM or environment.",
        }
    }

    pub(super) fn error(self, error: Error) -> String {
        let operation = self.operation();
        let Error::Api {
            status,
            code,
            request_id,
        } = &error
        else {
            return format!("{operation}: {error}");
        };
        if status.as_u16() != 403 {
            return format!("{operation}: {}", super::api(error));
        }
        // Only expose documented codes and recognizable support IDs. Bodies,
        // messages and arbitrary header values can contain credentials.
        let advice = match code.as_deref() {
            Some("api_key_action_forbidden") => Some(self.permission_hint()),
            Some("api_key_sandbox_forbidden") => Some("The key’s scope does not include this VM or environment. Check the allowed resources in Boat → API Keys."),
            Some("environment_forbidden") => Some("Ask a Boat organization owner or admin to grant access to the checkout environment."),
            Some("api_key_org_mismatch" | "api_key_org_forbidden") => Some("Check that this Boat key is allowed to use the selected organization and environment."),
            Some("api_key_expired") => Some("Reconnect a current Boat key in Settings → Remote machines."),
            Some("trial_machine_class_not_allowed") => Some("Choose Small or Standard for a Boat trial."),
            Some("machine_class_plan_required") => Some("Choose a VM size available on this Boat plan. See boat limits for the available sizes."),
            _ => None,
        };
        let mut text = format!("{operation}: Boat returned HTTP {status}");
        if let (Some(code), Some(_)) = (code, advice) {
            let _ = write!(text, " ({code})");
        }
        let _ = write!(
            text,
            ". {}",
            advice.unwrap_or_else(|| self.permission_hint())
        );
        if let Some(id) = request_id.as_deref().filter(|id| support_id(id)) {
            let _ = write!(text, " Request ID: {id}.");
        }
        text
    }
}

fn support_id(id: &str) -> bool {
    (id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
        || id.strip_prefix("req_").is_some_and(|suffix| {
            (1..=64).contains(&suffix.len())
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
