use tonic::{Code, Status};

/// The final status of a gRPC call. Statuses other than OK are completed
/// calls too, like HTTP error responses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcStatus {
    pub code: i32,
    pub message: String,
}

impl GrpcStatus {
    pub fn ok() -> Self {
        Self {
            code: 0,
            message: String::new(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.code == 0
    }

    /// The canonical name, such as `OK` or `UNAVAILABLE`.
    pub fn name(&self) -> &'static str {
        match Code::from_i32(self.code) {
            Code::Ok => "OK",
            Code::Cancelled => "CANCELLED",
            Code::Unknown => "UNKNOWN",
            Code::InvalidArgument => "INVALID_ARGUMENT",
            Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Code::NotFound => "NOT_FOUND",
            Code::AlreadyExists => "ALREADY_EXISTS",
            Code::PermissionDenied => "PERMISSION_DENIED",
            Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Code::FailedPrecondition => "FAILED_PRECONDITION",
            Code::Aborted => "ABORTED",
            Code::OutOfRange => "OUT_OF_RANGE",
            Code::Unimplemented => "UNIMPLEMENTED",
            Code::Internal => "INTERNAL",
            Code::Unavailable => "UNAVAILABLE",
            Code::DataLoss => "DATA_LOSS",
            Code::Unauthenticated => "UNAUTHENTICATED",
        }
    }
}

impl From<&Status> for GrpcStatus {
    fn from(status: &Status) -> Self {
        Self {
            code: status.code() as i32,
            message: status.message().to_owned(),
        }
    }
}

impl std::fmt::Display for GrpcStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.code, self.name())?;

        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }

        Ok(())
    }
}
